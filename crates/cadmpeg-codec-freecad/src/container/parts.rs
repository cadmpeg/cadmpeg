// SPDX-License-Identifier: Apache-2.0
//! Optional member opening under actual archive and compressed-member scopes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

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
            ctx.decode_part(stage, frame.header(), header_spec(ctx, frame)?, |_stage| {
                archive.read_member_header(ctx, frame)
            })?;
        let Some(payload) = payload else {
            members.failed.insert(name.to_owned());
            continue;
        };
        match ctx.decode_part(stage, payload.view(), spec("zip.entry")?, |_stage| {
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
    pub(super) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
}

pub(super) fn document<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    stage: &mut PartOutput,
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
    let payload = ctx.decode_part(stage, frame.header(), header_spec(ctx, frame)?, |_stage| {
        archive.read_member_header(ctx, frame)
    })?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let result = ctx.decode_part(stage, payload.view(), spec("zip.entry")?, |member| {
        let view = archive.open_member(ctx, payload)?;
        ctx.decode_part(member, view, spec("freecad.document_xml")?, |_document| {
            let xml = super::admit_document(ctx, view.window())?;
            if let Some((_, count)) = super::xml_envelope_counts(view.window()) {
                ctx.charge_entities(count, "admit FCStd document objects")?;
            }
            let mut losses = Vec::new();
            let source = crate::persistence::source::Source::new(ctx, view, &xml)?;
            let (facts, schema) = source.record(
                &crate::native::native_id("document", "0"),
                xml.root_element(),
                || super::parse_document_impl(&xml, &mut losses, Some(&source)),
            )?;
            Ok(Document {
                view,
                xml,
                facts,
                schema,
                losses,
            })
        })
    })?;
    Ok(result.flatten())
}

pub(super) fn keep_readable_entries(
    ctx: &DecodeContext<'_>,
    archive: &ArchiveSnapshot<'_>,
    opened: &OptionalMembers<'_>,
    stage: &mut PartOutput,
) -> Result<BTreeMap<String, BTreeMap<String, u64>>, CodecError> {
    let mut pointers = BTreeMap::new();
    for entry in archive.entries() {
        let Some(view) = opened.data.get(&entry.name).copied() else {
            continue;
        };
        let id = crate::native::native_id("entry", &entry.name);
        let record = crate::native::EntryRecord::new(
            id.clone(),
            entry.name.clone(),
            super::classify(&entry.name),
            Vec::new(),
        )?;
        ctx.record(&id, view, || stage.insert_native(ctx, "entries", record))?;
        pointers.insert(
            id,
            crate::source_archive::entry_pointers(entry, u64_from_index(view.window().len()))?,
        );
    }
    Ok(pointers)
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
) -> Result<(), CodecError> {
    let (_, mut native, source, mut diagnostics) = output.into_parts(ctx)?.into_parts();
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
