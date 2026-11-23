// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds one manifest-selected root and one member per document.
//! [`decode`] classifies every member, decodes the model root, and delegates
//! occurrence-scoped graph composition to [`merge`].

use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;

mod archive;
pub(crate) mod merge;
pub(crate) mod observer;
mod session;

pub use observer::{observe_f3z_members, F3zMemberObserver, F3zPhase};
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
    let classified = archive::classify_members(ctx, scan, false)?;
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
        mut scene_sources,
    ) = session.take_member(ctx, &model_root, members)?;
    let source = ir
        .source
        .take()
        .ok_or_else(|| CodecError::malformed("F3Z root has no source metadata"))?;
    inline_given_up_parts(ctx, &mut fidelity, &model_root)?;
    fidelity.decode_digest = Sha256Digest::digest(scan.source_image);
    let root_untyped = fidelity
        .remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .map(|image| image.untyped().clone())
        .unwrap_or_default();
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)])?;
    // The archive image holds the root member's bytes, and with them what that
    // member's image left undecoded. At container depth no member is decoded,
    // so it is marked with every class a member's parts can hold.
    let archive_untyped = if ctx.container_only() {
        [ArenaClass::Model, ArenaClass::Design].into()
    } else {
        root_untyped
    };
    if !archive_untyped.is_empty() {
        fidelity.mark_untyped(crate::ids::FILE_SOURCE_IMAGE_ID, archive_untyped)?;
    }
    crate::decode::commit_given_up_parts(ctx, scan, &mut ir, &mut fidelity)?;
    if let Some(drawing_root) = &session.omitted_drawing_root {
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
        members.clear();
        report.losses.append(&mut outer_losses);
        return finalize_result(
            ctx,
            ir,
            source,
            report,
            fidelity,
            &merge::InstanceScopes::new(),
        );
    }

    let (merged, instance_scopes) = merge::merge_archive(
        ctx,
        session,
        members,
        (operation_poses, &mut scene_sources),
        &mut ir,
        &mut report,
        &mut fidelity,
    )?;
    if merged > 0 {
        inline_given_up_parts(ctx, &mut fidelity, "")?;
        fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID);
        report.notes.push(format!(
            "{merged} resolved external occurrence(s) retain shared member definitions, native records, and source bytes"
        ));
    }
    report.notes.push(format!(
        "merged {merged} external occurrence(s) from the f3z archive"
    ));
    crate::scene::bind(
        ctx,
        &mut ir,
        &scene_sources,
        &instance_scopes,
        &mut report.losses,
    )?;
    report.losses.append(&mut outer_losses);
    finalize_result(ctx, ir, source, report, fidelity, &instance_scopes)
}

/// Gives each given-up part in `stream` that keeps only a digest a copy of
/// its bytes from the source image record, before that record is removed.
fn inline_given_up_parts(
    ctx: &DecodeContext<'_>,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
    stream: &str,
) -> Result<(), CodecError> {
    let Some(image) = fidelity
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .and_then(cadmpeg_ir::source_fidelity::RetainedSourceRecord::data)
    else {
        return Ok(());
    };
    let mut inlined = Vec::new();
    for (id, record) in fidelity.retained_records() {
        if record.data().is_some()
            || record.stream() != stream
            || !id.as_str().contains(":source:part#")
        {
            continue;
        }
        let bytes = usize::try_from(record.offset())
            .ok()
            .zip(usize::try_from(record.end_offset()).ok())
            .and_then(|(start, end)| image.get(start..end))
            .ok_or_else(|| CodecError::malformed("given-up F3D part escapes its source image"))?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes.len()),
            "given-up part source bytes",
        )?;
        let mut data = ctx.alloc_filled(bytes.len(), 0_u8, "given-up part source bytes")?;
        data.copy_from_slice(bytes);
        let retained = cadmpeg_ir::source_fidelity::RetainedSourceRecord::from_bytes(
            stream.to_owned(),
            record.offset(),
            cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data },
        )?
        .with_untyped(record.untyped().iter().copied());
        inlined.push((id.clone(), retained));
    }
    for (id, retained) in inlined {
        fidelity.remove_retained_record(id.as_str());
        fidelity.insert_retained_record(id, retained)?;
    }
    Ok(())
}

fn finalize_result(
    ctx: &DecodeContext<'_>,
    mut ir: cadmpeg_ir::CadIr,
    mut source: cadmpeg_ir::SourceMeta,
    mut body: DecodeBody,
    source_fidelity: cadmpeg_ir::SourceFidelity,
    instance_scopes: &merge::InstanceScopes,
) -> Result<Decoded, CodecError> {
    body.losses
        .extend(crate::tree::own_once(ctx, &mut ir, instance_scopes)?);
    crate::design::owners::release_unbound(&mut ir.model);
    body.losses
        .extend(crate::tree::state_root_definition(ctx, &mut ir)?);
    let depth = body.transfer.depth();
    cadmpeg_ir::design::projection::project_layer(
        ctx,
        "f3d",
        &mut ir,
        &mut body.losses,
        |ir, losses| crate::design_v11::project(ctx, ir, depth, losses),
    )?;
    ir.finalize();
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
    )?;
    source.attributes.insert(
        cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
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
