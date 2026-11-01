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
use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;

mod archive;
pub(crate) mod merge;
mod observer;
mod session;

pub use observer::{observe_f3z_members, F3zMemberObserver};
pub use session::{F3zDecodedMember, F3zMember, F3zMemberKind, F3zSession};

fn push_loss(losses: &mut Vec<LossNote>, code: F3dLossCode, args: std::fmt::Arguments<'_>) {
    losses.push(code.note(args.to_string()));
}

/// Inspects every document member under the F3Z archive identity.
pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ContainerSummary, CodecError> {
    let (model_root, _) = archive::model_root(ctx, scan)?;
    scan.has_entry(&model_root).then_some(()).ok_or_else(|| {
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
    let session = F3zSession::open_scanned(ctx, scan.fork())?;
    compose(ctx, &session, &mut std::collections::BTreeMap::new())
}

fn compose(
    ctx: &DecodeContext<'_>,
    session: &F3zSession<'_>,
    members: &mut std::collections::BTreeMap<String, F3zDecodedMember>,
) -> Result<Decoded, CodecError> {
    let scan = &session.scan;
    let model_root = session.model_root.clone();
    let mut outer_losses = session.archive.losses.clone();
    let (
        Decoded {
            mut ir,
            body: mut report,
            source_fidelity: mut fidelity,
        },
        operation_poses,
    ) = session.take_member(ctx, &model_root, members)?;
    let source = ir
        .source
        .take()
        .ok_or_else(|| CodecError::malformed("F3Z root has no source metadata"))?;
    if let Some(root_image) = fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID) {
        let id = cadmpeg_ir::ids::UnknownId::mint("f3d:file:source-image#root")
            .map_err(CodecError::malformed)?;
        fidelity.insert_retained_record(id, root_image)?;
    }
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)])?;
    if session.archive.members.values().any(|member| {
        matches!(member, archive::ClassifiedMember::Unreadable(_))
    }) {
        fidelity.mark_untyped(
            crate::ids::FILE_SOURCE_IMAGE_ID,
            [
                cadmpeg_ir::document::ArenaClass::Model,
                cadmpeg_ir::document::ArenaClass::Design,
            ],
        )?;
    }
    if let Some(drawing_root) = &session.omitted_drawing_root {
        fidelity.mark_untyped(
            crate::ids::FILE_SOURCE_IMAGE_ID,
            [cadmpeg_ir::document::ArenaClass::Model],
        )?;
        push_loss(&mut report.losses, F3dLossCode::DrawingDocumentOmitted, format_args!(
            "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
        ));
    }
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    report.notes.push(format!(
        "f3z archive: {member_count} document member(s); root {model_root}"
    ));
    if ctx.container_only() {
        let mut untyped = cadmpeg_ir::document::ArenaClasses::default();
        for member in session.members() {
            match member.kind() {
                session::F3zMemberKind::Design
                | session::F3zMemberKind::Skipped(_) => {
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Model);
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Design);
                }
                session::F3zMemberKind::PlacementOnly => {
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Design);
                }
                session::F3zMemberKind::Drawing => {
                    untyped.insert(cadmpeg_ir::document::ArenaClass::Model);
                }
            }
        }
        crate::decode::declare_source_untyped(
            &mut fidelity,
            crate::ids::FILE_SOURCE_IMAGE_ID,
            untyped,
        )?;
        members.clear();
        report.losses.append(&mut outer_losses);
        return finalize_result(ctx, ir, source, report, fidelity);
    }

    let merged = merge::merge_archive(
        ctx,
        session,
        members,
        operation_poses,
        &mut ir,
        &mut report,
        &mut fidelity,
    )?;
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
    report.losses.append(&mut outer_losses);
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
    crate::design::owners::release_unbound(&mut ir.model);
    ir.finalize();
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
    )?;
    source_fidelity.decode_digest = Some(
        cadmpeg_ir::hash::digest::Sha256Digest::try_from(hash)
            .map_err(CodecError::malformed)?,
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
