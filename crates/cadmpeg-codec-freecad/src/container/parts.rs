// SPDX-License-Identifier: Apache-2.0
//! Optional member opening under actual archive and compressed-member scopes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartSpec};

#[derive(Default)]
pub(super) struct OptionalMembers<'source> {
    pub(super) data: BTreeMap<String, View<'source>>,
    pub(super) failed: BTreeSet<String>,
}

pub(super) fn open_optional<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    output: &mut PartOutput,
) -> Result<OptionalMembers<'source>, CodecError> {
    let frames = archive.member_frames(ctx)?;
    for entry in archive.entries() {
        if !crate::native::is_safe_entry_name(&entry.name) {
            return Err(CodecError::malformed(format_args!(
                "unsafe ZIP entry path {:?}",
                entry.name
            )));
        }
    }
    let stage = output;
    let mut members = OptionalMembers::default();
    for (entry, frame) in archive.entries().iter().zip(&frames) {
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
        ctx.retain_bytes(
            u64_from_index(
                name.len()
                    .checked_add(size_of::<(String, View<'_>)>())
                    .ok_or_else(|| CodecError::malformed("optional ZIP member index overflows"))?,
            ),
            "FreeCAD optional member source index",
        )?;
        let payload =
            ctx.decode_part(stage, frame.header(), part("zip.local_header"), |_stage| {
                archive.read_member_header(frame)
            })?;
        let Some(payload) = payload else {
            members.failed.insert(name.to_owned());
            continue;
        };
        match ctx.decode_part(stage, payload.view(), part("zip.entry"), |_stage| {
            archive.open_member(ctx, payload)
        })? {
            Some(view) => {
                members.data.insert(name.to_owned(), view);
            }
            None => {
                members.failed.insert(name.to_owned());
            }
        }
    }
    Ok(members)
}

pub(super) struct Document<'source> {
    pub(super) view: View<'source>,
    pub(super) xml: roxmltree::Document<'source>,
    pub(super) facts: crate::native::DocumentFacts,
    pub(super) schema: String,
    pub(super) root_properties: Option<crate::persistence::document_properties::RootProperties>,
    pub(super) object_declarations: Option<crate::persistence::object_parts::ObjectDeclarations>,
    pub(super) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(super) fn document<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    stage: &mut PartOutput,
    found: &mut bool,
) -> Result<Option<Document<'source>>, CodecError> {
    let index = archive
        .entries()
        .iter()
        .position(|entry| entry.name == "Document.xml")
        .ok_or_else(|| CodecError::WrongFormat("ZIP has no root Document.xml".into()))?;
    let frames = archive.member_frames(ctx)?;
    let frame = frames[index].ok_or_else(|| {
        CodecError::malformed("Document.xml has no provable local-header and payload boundary")
    })?;
    let payload = ctx.decode_part(stage, frame.header(), part("zip.local_header"), |_stage| {
        archive.read_member_header(frame)
    })?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let result = ctx.decode_part(stage, payload.view(), part("zip.entry"), |member| {
        let view = archive.open_member(ctx, payload)?;
        *found = true;
        let document = ctx
            .decode_part(
                member,
                view,
                crate::persistence::document_part("freecad.document_xml"),
                |document| {
                    let xml = super::admit_document(ctx, view.window())?;
                    if let Some((_, count)) = super::xml_envelope_counts(view.window()) {
                        ctx.charge_entities(count, "admit FCStd document objects")?;
                    }
                    let mut losses = Vec::new();
                    let source = crate::persistence::source::Source::new(view, &xml)?;
                    let root = xml.root_element();
                    if !root.has_tag_name("Document") {
                        return Err(CodecError::WrongFormat(
                            "Document.xml root is not Document".into(),
                        ));
                    }
                    let extent = crate::fidelity::relative_xml_extent(view, root.range())?;
                    let facts = ctx.decode_part(
                        document,
                        extent,
                        crate::persistence::document_part("freecad.document"),
                        |root_stage| {
                            let (facts, schema) =
                                super::parse_document_impl(&xml, &mut losses, Some(&source))?;
                            root_stage.insert_native(ctx, "document", facts.clone())?;
                            let root_properties = crate::persistence::document_properties::decode(
                                ctx, view, &xml, root_stage,
                            )?;
                            let object_declarations = crate::persistence::object_parts::decode(
                                ctx,
                                view,
                                &xml,
                                crate::persistence::Vocabulary::from_declaration(&schema)?,
                                root_stage,
                            )?;
                            Ok((facts, schema, root_properties, object_declarations))
                        },
                    )?;
                    Ok(
                        facts.map(|(facts, schema, root_properties, object_declarations)| {
                            Document {
                                view,
                                xml,
                                facts,
                                schema,
                                root_properties,
                                object_declarations,
                                losses,
                            }
                        }),
                    )
                },
            )
            .map(Option::flatten)?;
        if document.is_none() {
            keep_entry(ctx, &archive.entries()[index], member)?;
        }
        Ok(document)
    })?;
    Ok(result.flatten())
}

pub(super) fn keep_readable_entries(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    opened: &OptionalMembers<'_>,
    stage: &mut PartOutput,
) -> Result<(), CodecError> {
    for entry in archive.entries() {
        if opened.data.contains_key(&entry.name) {
            keep_entry(ctx, entry, stage)?;
        }
    }
    Ok(())
}

fn keep_entry(
    ctx: &DecodeContext<'_>,
    entry: &cadmpeg_container::EntryRecord,
    stage: &mut PartOutput,
) -> Result<(), CodecError> {
    let record = crate::native::EntryRecord::new(
        crate::native::native_id("entry", &entry.name),
        entry.name.clone(),
        super::classify(&entry.name),
        Vec::new(),
    )?;
    stage.insert_native(ctx, "entries", record)
}

/// A FreeCAD part; its bytes can hold content of either class.
pub(crate) fn part(label: &'static str) -> PartSpec {
    PartSpec::new(label).holding([
        cadmpeg_ir::ArenaClass::Model,
        cadmpeg_ir::ArenaClass::Design,
    ])
}

pub(crate) fn append(
    ctx: &DecodeContext<'_>,
    output: PartOutput,
    ir: &mut cadmpeg_ir::CadIr,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let (model, mut native, source, mut diagnostics) = output.into_parts();
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
    Ok(())
}

#[cfg(test)]
mod tests;
