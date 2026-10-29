// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds one manifest-selected root and one member per document.
//! [`decode`] classifies every member, decodes the model root, and delegates
//! occurrence-scoped graph composition to [`merge`].

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;
use crate::decode::AuthoredDecoded;
use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;

mod archive;
pub(crate) mod merge;

fn push_loss(losses: &mut Vec<LossNote>, code: F3dLossCode, args: std::fmt::Arguments<'_>) {
    losses.push(code.note(args.to_string()));
}

/// Inspects every document member under the F3Z archive identity.
pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ContainerSummary, CodecError> {
    let (model_root, _) = archive::model_root(ctx, scan)?;
    scan.entry_view(&model_root).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "f3z root member {model_root} is not present in the archive"
        ))
    })?;
    let classified = archive::classify_members(ctx, scan)?;
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    let mut notes = Vec::new();
    notes.push(format!(
        "f3z archive: {member_count} document member(s); model root {model_root}"
    ));
    Ok(ContainerSummary::classified(
        classified.layers,
        cadmpeg_ir::ContainerKind::Zip,
        scan.entries.clone(),
        classified.losses,
        notes,
    ))
}

/// Decodes a scanned `.f3z` archive into one occurrence-scoped document.
pub(crate) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<Decoded, CodecError> {
    let (model_root, omitted_drawing_root) = archive::model_root(ctx, scan)?;
    let mut outer = archive::classify_members(ctx, scan)?;
    let root_scan = outer.member_scan(&model_root)?;
    let mut root = crate::decode::decode_archive_member(ctx, root_scan, &outer.layers)?;
    if let Some(root_image) = root
        .source_fidelity
        .remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
    {
        let id = cadmpeg_ir::ids::UnknownId::mint("f3d:file:source-image#root")
            .map_err(CodecError::malformed)?;
        root.source_fidelity
            .insert_retained_record(id, root_image)?;
    }
    root.source_fidelity
        .retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)])?;
    if outer
        .members
        .values()
        .any(|member| matches!(member, archive::ClassifiedMember::Unreadable(_)))
    {
        root.source_fidelity.mark_untyped(
            crate::ids::FILE_SOURCE_IMAGE_ID,
            [
                cadmpeg_ir::document::ArenaClass::Model,
                cadmpeg_ir::document::ArenaClass::Design,
            ],
        )?;
    }
    if let Some(drawing_root) = omitted_drawing_root {
        root.source_fidelity.mark_untyped(
            crate::ids::FILE_SOURCE_IMAGE_ID,
            [cadmpeg_ir::document::ArenaClass::Model],
        )?;
        push_loss(&mut root.body.losses, F3dLossCode::DrawingDocumentOmitted, format_args!(
            "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
        ));
    }
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    root.body.notes.push(format!(
        "f3z archive: {member_count} document member(s); root {model_root}"
    ));
    if ctx.container_only() {
        let mut untyped = cadmpeg_ir::document::ArenaClasses::default();
        for member in outer.members.values() {
            match member {
                archive::ClassifiedMember::Scanned(member) => {
                    if crate::container::design_breps(member).next().is_some()
                        || crate::container::text_brep_names(member).next().is_some()
                    {
                        untyped.insert(cadmpeg_ir::document::ArenaClass::Model);
                    }
                    if member.entries.iter().any(|entry| {
                        member.is_design_stream(
                            entry,
                            cadmpeg_core::container::ContainerRole::Bulkstream,
                        )
                    }) {
                        untyped.insert(cadmpeg_ir::document::ArenaClass::Design);
                    }
                }
                archive::ClassifiedMember::Unreadable(_) => {
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Model);
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Design);
                }
            }
        }
        crate::decode::declare_source_untyped(
            &mut root.source_fidelity,
            crate::ids::FILE_SOURCE_IMAGE_ID,
            untyped,
        )?;
        root.body.losses.append(&mut outer.losses);
        return finalize_result(ctx, root.ir, root.source, root.body, root.source_fidelity);
    }
    let merged = merge::merge_archive(ctx, scan, &outer, model_root, &mut root)?;
    let AuthoredDecoded {
        mut ir,
        source,
        body: mut report,
        source_fidelity: mut fidelity,
        ..
    } = root;
    if merged > 0 {
        if let Some(image) = fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID) {
            let id = cadmpeg_ir::ids::UnknownId::mint("f3d:file:source-image#archive")
                .map_err(CodecError::malformed)?;
            fidelity.insert_retained_record(id, image)?;
        }
        report.notes.push(format!(
            "{merged} resolved external occurrence(s) retain shared member definitions, native records, and source bytes"
        ));
    }
    report.notes.push(format!(
        "merged {merged} external occurrence(s) from the f3z archive"
    ));
    merge::make_sibling_ordinals_unique(&mut ir.model.occurrences)?;
    report.losses.append(&mut outer.losses);
    finalize_result(ctx, ir, source, report, fidelity)
}

fn finalize_result(
    ctx: &DecodeContext<'_>,
    mut ir: cadmpeg_ir::CadIr,
    source: cadmpeg_ir::SourceMeta,
    mut body: DecodeBody,
    mut source_fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<Decoded, CodecError> {
    body.transfer.scope = ir.scope;
    ir.finalize();
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
    )?;
    source_fidelity.metadata.insert(
        cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY.into(),
        hash,
    );
    ir.source = Some(source);
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
