// SPDX-License-Identifier: Apache-2.0
//! Optional member opening under actual archive and compressed-member scopes.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, RecordOwner, ResourceDimension, ResourceLimit,
    SourceExtentProof, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

pub(crate) type EntryOwners<'source> = BTreeMap<String, Rc<EntryOwner<'source>>>;

pub(crate) struct EntryOwner<'source> {
    owner: RecordOwner<'source>,
    archive: Rc<ArchiveSnapshot<'source>>,
    entry: usize,
    proof: SourceExtentProof,
}
impl<'source> std::ops::Deref for EntryOwner<'source> {
    type Target = RecordOwner<'source>;
    fn deref(&self) -> &Self::Target {
        &self.owner
    }
}
impl<'source> EntryOwner<'source> {
    pub(crate) fn new(
        ctx: &DecodeContext<'source>,
        owner: RecordOwner<'source>,
        archive: Rc<ArchiveSnapshot<'source>>,
        entry: usize,
    ) -> Result<Self, CodecError> {
        let declaration = archive
            .entries()
            .get(entry)
            .ok_or_else(|| CodecError::malformed("entry name has no admitted declaration"))?;
        let source = archive.entry_name_source(declaration)?;
        Ok(Self {
            owner,
            proof: ctx.source_extent_proof(source)?,
            archive,
            entry,
        })
    }
    pub(crate) fn read_name(&self, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
        ctx.require_source_authority(&self.proof)?;
        let entry = &self.archive.entries()[self.entry];
        ctx.require_source_ancestry(&self.proof, self.archive.entry_name_source(entry)?)?;
        self.archive.read_entry_name(ctx, entry)
    }
}

pub(crate) fn archive_source<'source>(
    ctx: &DecodeContext<'source>,
    root: View<'source>,
) -> Result<Rc<ArchiveSnapshot<'source>>, CodecError> {
    let archive = ArchiveSnapshot::new(ctx, root)?;
    Ok(Rc::new(archive))
}

#[derive(Default)]
pub(super) struct OptionalMembers<'source> {
    pub(super) data: BTreeMap<String, View<'source>>,
    pub(super) failed: BTreeSet<String>,
    pub(super) parents: BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    pub(super) records: Vec<crate::native::EntryRecord>,
    pub(super) owners: EntryOwners<'source>,
}

pub(super) fn open_optional<'source>(
    ctx: &DecodeContext<'source>,
    archive: &Rc<ArchiveSnapshot<'source>>,
    output: &mut PartOutput,
) -> Result<OptionalMembers<'source>, CodecError> {
    let frames = archive.member_frames(ctx)?;
    let stage = output;
    let mut members = OptionalMembers::default();
    for (index, (entry, frame)) in archive.entries().iter().zip(&frames).enumerate() {
        if entry.name == "Document.xml" {
            continue;
        }
        let Some(frame) = *frame else {
            return Err(CodecError::malformed(format_args!(
                "ZIP member {} has no provable local-header and payload boundary",
                entry.name
            )));
        };
        let name = entry.name.as_str();
        let payload =
            ctx.decode_part(stage, frame.header(), header_spec(ctx, frame)?, |_header| {
                let id = crate::native::native_id("entry", name);
                let (value, owner) = ctx.record_owner(&id, frame.header(), || {
                    let payload = archive.read_member_header(ctx, frame)?;
                    let actual_name = archive.read_entry_name(ctx, entry)?;
                    if actual_name != name || !crate::native::is_safe_entry_name(&actual_name) {
                        return Err(CodecError::malformed(
                            "ZIP source name disagrees with its safe navigation identity",
                        ));
                    }
                    let role = super::classify(&actual_name);
                    let record = crate::native::EntryRecord::new(
                        crate::native::native_id("entry", &actual_name),
                        actual_name,
                        role,
                        Vec::new(),
                    )?;
                    Ok((payload, record))
                })?;
                Ok((value.0, value.1, owner))
            })?;
        let Some((payload, record, owner)) = payload else {
            members.failed.insert(source_key(ctx, name)?);
            continue;
        };
        retain_owner(
            ctx,
            record.name(),
            owner,
            archive,
            index,
            &mut members.owners,
        )?;
        members.records.push(record);
        let payload_view = payload.view();
        match ctx.decode_part(stage, payload_view, spec("zip.entry")?, |_stage| {
            archive.open_member(ctx, payload)
        })? {
            Some(view) => {
                let parent = stage.parent(ctx, payload_view, "zip.entry")?;
                members.parents.insert(source_key(ctx, name)?, parent);
                members.data.insert(source_key(ctx, name)?, view);
            }
            None => {
                members.failed.insert(source_key(ctx, name)?);
            }
        }
    }
    Ok(members)
}

fn source_key(ctx: &DecodeContext<'_>, name: &str) -> Result<String, CodecError> {
    let mut key = ctx.alloc_filled(name.len(), 0_u8, "FreeCAD archive entry source index key")?;
    key.copy_from_slice(name.as_bytes());
    String::from_utf8(key).map_err(CodecError::malformed)
}

fn retain_owner<'source>(
    ctx: &DecodeContext<'source>,
    name: &str,
    owner: RecordOwner<'source>,
    archive: &Rc<ArchiveSnapshot<'source>>,
    entry: usize,
    owners: &mut EntryOwners<'source>,
) -> Result<(), CodecError> {
    let key = source_key(ctx, name)?;
    if owners
        .insert(
            key,
            Rc::new(EntryOwner::new(ctx, owner, Rc::clone(archive), entry)?),
        )
        .is_some()
    {
        return Err(CodecError::malformed(
            "duplicate archive entry source owner",
        ));
    }
    Ok(())
}

pub(super) fn incomplete_local_header<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let Some(header) = archive.recovery_header()? else {
        return Ok(());
    };
    let capture = ctx.capture_part_extent(header)?;
    let mut framing = ctx.alloc_filled(4, 0..0, "incomplete ZIP header framing")?;
    let mut fields = 0;
    for (start, end) in [(0, 4), (6, 8), (18, 22), (26, 30)] {
        let end = end.min(header.window().len());
        if start < end {
            framing[fields] =
                capture.offset() + u64_from_index(start)..capture.offset() + u64_from_index(end);
            fields += 1;
        }
    }
    framing.truncate(fields);
    let level = PartLevel::parse("zip.local_header")
        .ok_or_else(|| CodecError::malformed("ZIP header level is absent from Appendix C"))?;
    ctx.decode_part(output, header, PartSpec::new(level, framing), |_stage| {
        archive.read_recovery_header(ctx)
    })?;
    Ok(())
}

pub(super) struct Document<'source> {
    pub(super) view: View<'source>,
    pub(super) facts: crate::native::DocumentFacts,
    pub(super) schema: String,
    pub(super) source_whitespace: Vec<View<'source>>,
    pub(super) root_properties: Option<crate::persistence::document_properties::RootProperties>,
    pub(super) object_declarations:
        Option<crate::persistence::object_parts::ObjectDeclarations<'source>>,
    pub(super) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(super) struct DocumentRead<'source> {
    pub(super) record: Option<crate::native::EntryRecord>,
    pub(super) document: Option<Document<'source>>,
    pub(super) view: Option<View<'source>>,
    pub(super) pointers: BTreeMap<String, BTreeMap<String, u64>>,
}

pub(super) fn document<'source>(
    ctx: &DecodeContext<'source>,
    archive: &Rc<ArchiveSnapshot<'source>>,
    stage: &mut PartOutput,
    entries: &BTreeMap<String, View<'source>>,
    parents: &BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    records: &[crate::native::EntryRecord],
    owners: &mut EntryOwners<'source>,
) -> Result<DocumentRead<'source>, CodecError> {
    let index = archive
        .entries()
        .iter()
        .position(|entry| entry.name == "Document.xml")
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let frames = archive.member_frames(ctx)?;
    let frame = frames[index].ok_or_else(|| {
        CodecError::malformed("Document.xml has no provable local-header and payload boundary")
    })?;
    let payload = ctx.decode_part(stage, frame.header(), header_spec(ctx, frame)?, |_header| {
        let id = crate::native::native_id("entry", "Document.xml");
        let (value, owner) = ctx.record_owner(&id, frame.header(), || {
            let payload = archive.read_member_header(ctx, frame)?;
            let actual_name = archive.read_entry_name(ctx, &archive.entries()[index])?;
            if actual_name != "Document.xml" {
                return Err(CodecError::malformed(
                    "Document.xml source name disagrees with its navigation identity",
                ));
            }
            let role = super::classify(&actual_name);
            let record = crate::native::EntryRecord::new(
                crate::native::native_id("entry", &actual_name),
                actual_name,
                role,
                Vec::new(),
            )?;
            Ok((payload, record))
        })?;
        Ok((value.0, value.1, owner))
    })?;
    let Some((payload, record, owner)) = payload else {
        return Ok(DocumentRead {
            record: None,
            document: None,
            view: None,
            pointers: BTreeMap::new(),
        });
    };
    retain_owner(ctx, record.name(), owner, archive, index, owners)?;
    let mut declared_entries = declared_entries(records);
    declared_entries.insert(record.name().to_owned());
    let result = ctx.decode_part(stage, payload.view(), spec("zip.entry")?, |member| {
        let view = archive.open_member(ctx, payload)?;
        let document = ctx
            .decode_part(member, view, spec("freecad.document_xml")?, |document| {
                let source = std::rc::Rc::new(crate::xml_framing::XmlSource::new(ctx, view)?);
                if source
                    .elements()
                    .iter()
                    .filter(|element| element.parent().is_none())
                    .count()
                    != 1
                {
                    return Err(CodecError::malformed(
                        "Document.xml does not have one root element",
                    ));
                }
                let root = source
                    .elements()
                    .iter()
                    .position(|element| element.parent().is_none())
                    .ok_or_else(|| CodecError::malformed("Document.xml has no framed root"))?;
                if source.name(root)? != "Document" {
                    return Err(CodecError::WrongFormat(
                        "Document.xml root is not Document".into(),
                    ));
                }
                let mut losses = Vec::new();
                let facts = ctx.decode_part(
                    document,
                    source.extent(root)?,
                    source.spec(root, "freecad.document")?,
                    |root_stage| {
                        source.validate(root)?;
                        if !source.peek_text(ctx, root)?.trim().is_empty() {
                            return Err(CodecError::malformed(
                                "Document has non-whitespace direct text",
                            ));
                        }
                        let id = crate::native::native_id("document", "0");
                        ctx.record(&id, source.extent(root)?, || {
                            let (mut facts, schema) =
                                super::document_facts(ctx, &source, root, &mut losses)?;
                            let vocabulary =
                                crate::persistence::Vocabulary::from_declaration(&schema)?;
                            let context = crate::element_map::adopted::Context::new(
                                entries,
                                facts.file_version.value(),
                            )
                            .with_parents(parents)
                            .with_entry_owners(owners)
                            .with_declared_entries(&declared_entries);
                            for child in source.children(root) {
                                if matches!(source.name(child)?, "StringHasher" | "StringHasher2") {
                                    ctx.decode_part(
                                        root_stage,
                                        source.extent(child)?,
                                        source.spec(child, "freecad.string_hasher")?,
                                        |stage| {
                                            crate::element_map::adopted::decode(
                                                ctx, &source, child, None, None, &context, stage,
                                            )
                                        },
                                    )?;
                                }
                            }
                            let mut root_properties =
                                crate::persistence::document_properties::prepare_scanned(
                                    ctx, &source, root, &context, root_stage,
                                )?;
                            let mut object_declarations =
                                crate::persistence::object_parts::decode_scanned(
                                    ctx, &source, root, vocabulary, &context, root_stage,
                                )?;
                            crate::persistence::object_parts::finish_properties(
                                ctx,
                                &source,
                                &context,
                                &mut root_properties,
                                &mut object_declarations,
                                root_stage,
                            )?;
                            facts.domains = object_declarations
                                .sources
                                .values()
                                .map(|source| source.domain(ctx))
                                .collect::<Result<Vec<_>, _>>()?
                                .into_iter()
                                .flatten()
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                            root_stage.insert_native(ctx, "document", facts.clone())?;
                            let mut source_whitespace = context.take_source_whitespace();
                            crate::xml_framing::append_whitespace(
                                ctx,
                                &mut source_whitespace,
                                source.insignificant(ctx)?,
                            )?;
                            Ok((
                                facts,
                                schema,
                                root_properties,
                                Some(object_declarations),
                                source_whitespace,
                            ))
                        })
                    },
                )?;
                Ok(facts.map(
                    |(facts, schema, root_properties, object_declarations, source_whitespace)| {
                        Document {
                            view,
                            facts,
                            schema,
                            source_whitespace,
                            root_properties,
                            object_declarations,
                            losses,
                        }
                    },
                ))
            })
            .map(Option::flatten)?;
        let mut pointers = BTreeMap::new();
        if document.is_none() {
            let (id, fields) = keep_entry(&archive.entries()[index], view)?;
            pointers.insert(id, fields);
        }
        Ok((document, pointers, view))
    })?;
    let (document, pointers, view) = match result {
        Some((document, pointers, view)) => (document, pointers, Some(view)),
        None => (None, BTreeMap::new(), None),
    };
    Ok(DocumentRead {
        record: Some(record),
        document,
        view,
        pointers,
    })
}

pub(super) fn keep_readable_entries(
    archive: &ArchiveSnapshot<'_>,
    opened: &OptionalMembers<'_>,
) -> Result<BTreeMap<String, BTreeMap<String, u64>>, CodecError> {
    let mut pointers = BTreeMap::new();
    for entry in archive.entries() {
        let Some(view) = opened.data.get(&entry.name).copied() else {
            continue;
        };
        let (id, fields) = keep_entry(entry, view)?;
        pointers.insert(id, fields);
    }
    Ok(pointers)
}

pub(crate) fn opaque_side_entries(
    ctx: &DecodeContext<'_>,
    scan: &super::Scan<'_>,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    opaque_members(ctx, &scan.data, &scan.entry_parents, output)
}

pub(crate) fn declared_entries(records: &[crate::native::EntryRecord]) -> BTreeSet<String> {
    records
        .iter()
        .map(|entry| entry.name().to_owned())
        .collect()
}

pub(crate) fn merge_entry_contents(
    ctx: &DecodeContext<'_>,
    records: &mut [crate::native::EntryRecord],
    updates: impl IntoIterator<Item = crate::native::entry_contents::EntryContentUpdate>,
) -> Result<(), CodecError> {
    for update in updates {
        let entry = records
            .iter_mut()
            .find(|entry| entry.id() == update.entry_id())
            .ok_or_else(|| {
                CodecError::malformed("entry contents have no admitted header producer")
            })?;
        update.apply(ctx, entry)?;
    }
    Ok(())
}

pub(crate) fn stage_entries(
    ctx: &DecodeContext<'_>,
    records: Vec<crate::native::EntryRecord>,
    properties: &[crate::native::PropertyRecord],
    gui: &crate::gui::Graph,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    for mut entry in records {
        for (name, owner) in properties
            .iter()
            .flat_map(|property| {
                property
                    .side_entries()
                    .iter()
                    .map(move |name| (name, property.id.as_str()))
            })
            .chain(gui.properties.iter().flat_map(|property| {
                property
                    .side_entries
                    .iter()
                    .map(move |name| (name, property.id.as_str()))
            }))
            .chain(gui.documents.iter().flat_map(|document| {
                document.states.iter().flat_map(|state| {
                    state
                        .side_entries
                        .iter()
                        .map(move |name| (name, state.id.as_str()))
                })
            }))
        {
            if name == entry.name() {
                entry.add_reference(owner)?;
            }
        }
        output.insert_native(ctx, "entries", entry)?;
    }
    Ok(())
}

pub(super) fn opaque_members(
    ctx: &DecodeContext<'_>,
    sources: &BTreeMap<String, View<'_>>,
    parents: &BTreeMap<String, cadmpeg_ir::source_fidelity::parts::PartParent>,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    for (name, source) in sources {
        let Some(parent) = parents.get(name) else {
            continue;
        };
        if !output.has_descendant(parent)? {
            ctx.decode_part(
                output,
                *source,
                spec("freecad.opaque_side_entry")?.with_parent(parent),
                |_stage| Ok(()),
            )?;
        }
    }
    Ok(())
}

fn keep_entry(
    entry: &cadmpeg_container::EntryRecord,
    view: View<'_>,
) -> Result<(String, BTreeMap<String, u64>), CodecError> {
    let id = crate::native::native_id("entry", &entry.name);
    Ok((
        id,
        crate::source_archive::entry_pointers(entry, u64_from_index(view.window().len()))?,
    ))
}

pub(super) fn archive_spec() -> Result<PartSpec, CodecError> {
    spec("zip.archive")
}

fn header_spec(
    ctx: &DecodeContext<'_>,
    frame: cadmpeg_container::MemberFrame<'_>,
) -> Result<PartSpec, CodecError> {
    let fields = frame.header_framing()?;
    let bytes = fields
        .len()
        .checked_mul(size_of::<std::ops::Range<u64>>())
        .ok_or_else(|| CodecError::malformed("ZIP local-header framing storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), "FreeCAD local-header framing")?;
    let mut ranges = ctx.alloc_filled(fields.len(), 0..0, "FreeCAD local-header framing")?;
    for (range, field) in ranges.iter_mut().zip(fields) {
        *range = u64_from_index(field.start())..u64_from_index(field.end());
    }
    let level = PartLevel::parse("zip.local_header")
        .ok_or_else(|| CodecError::malformed("ZIP header level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, ranges))
}

fn spec(name: &str) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("ZIP member level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, Vec::new()))
}

pub(crate) fn append(
    ctx: &DecodeContext<'_>,
    output: PartOutput,
    ir: &mut cadmpeg_ir::CadIr,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<
    (
        Vec<String>,
        cadmpeg_ir::source_fidelity::parts::FinalizedFailureRegistry,
        cadmpeg_ir::source_fidelity::parts::PrunedPartCaptures,
    ),
    CodecError,
> {
    let ((model, mut native, source, mut diagnostics), dropped, mut failures, pruning) = output
        .into_parts(ctx)?
        .into_parts_with_failure_registry_and_capture_pruning(ctx)?;
    failures.offset_losses(losses.len())?;
    if model.entity_count() != 0 {
        let mut session = cadmpeg_ir::draft::CommitSession::new(&mut *ir, ctx, None)?;
        session.model_append_transaction(|stage| {
            let mut draft = cadmpeg_ir::draft::ModelDraft::new();
            *draft.model_mut() = model;
            stage.commit_model(draft)?.map_err(|error| match error {
                cadmpeg_ir::draft::DraftError::Resource(limit) => CodecError::ResourceLimit(limit),
                error => CodecError::malformed(error),
            })?;
            Ok(Some(()))
        })?;
    }
    for (name, mut records) in std::mem::take(native.namespace_mut("fcstd").arenas_mut()) {
        let destination = ir
            .native
            .namespace_mut("fcstd")
            .arenas_mut()
            .entry(name)
            .or_default();
        // The part helper charged these records when its root output received
        // them. This move is not a new retained copy.
        let additional = records
            .len()
            .checked_mul(size_of::<cadmpeg_ir::native::NativeRecord>())
            .map(u64_from_index)
            .ok_or_else(|| CodecError::malformed("FreeCAD shared part arena size overflows"))?;
        destination.try_reserve(records.len()).map_err(|_| {
            ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                additional,
                "FreeCAD shared part arena composition",
            )
        })?;
        destination.append(&mut records);
    }
    fidelity.append(source)?;
    losses.append(&mut diagnostics);
    Ok((dropped, failures, pruning))
}

#[cfg(test)]
mod tests;
