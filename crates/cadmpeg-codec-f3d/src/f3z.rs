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
mod merge;

fn push_note(
    ctx: &DecodeContext<'_>,
    notes: &mut Vec<String>,
    args: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "collect F3Z report notes";
    ctx.charge_collection_items(1, OPERATION)?;
    notes.try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, 1))?;
    notes.push(crate::container::format_retained(
        ctx,
        "retain F3Z report note",
        args,
    )?);
    Ok(())
}

fn push_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    args: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "collect F3Z report losses";
    ctx.charge_collection_items(1, OPERATION)?;
    losses.try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, 1))?;
    losses.push(code.note(crate::container::format_retained(
        ctx,
        "retain F3Z report loss",
        args,
    )?));
    Ok(())
}

fn append_losses(
    ctx: &DecodeContext<'_>,
    target: &mut Vec<LossNote>,
    mut incoming: Vec<LossNote>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "append F3Z report losses";
    let count = u64::try_from(incoming.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, u64::MAX))?;
    ctx.charge_collection_items(count, OPERATION)?;
    target.try_reserve(incoming.len())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, 0, count))?;
    target.append(&mut incoming);
    Ok(())
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
    push_note(ctx, &mut notes, format_args!(
        "f3z archive: {member_count} document member(s); model root {model_root}"
    ))?;
    Ok(ContainerSummary::classified(
        classified.layers,
        cadmpeg_ir::ContainerKind::Zip,
        crate::container::copy_summary_entries(ctx, &scan.entries)?,
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
    let outer = archive::classify_members(ctx, scan)?;
    let root_scan = outer.member_scan(&model_root)?;
    let AuthoredDecoded {
        mut ir,
        source,
        body: mut report,
        source_fidelity: mut fidelity,
    } = crate::decode::decode_archive_member(ctx, root_scan, &outer.layers)?;
    fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID);
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)])?;
    if let Some(drawing_root) = omitted_drawing_root {
        push_loss(ctx, &mut report.losses, F3dLossCode::DrawingDocumentOmitted, format_args!(
            "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
        ))?;
    }
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    push_note(ctx, &mut report.notes, format_args!(
        "f3z archive: {member_count} document member(s); root {model_root}"
    ))?;
    if ctx.container_only() {
        append_losses(ctx, &mut report.losses, outer.losses)?;
        return finalize_result(ctx, ir, source, report, fidelity);
    }

    let merged = merge::merge_archive(
        ctx,
        scan,
        &outer,
        model_root,
        &mut ir,
        &mut report,
        &mut fidelity,
    )?;
    if merged > 0 {
        fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID);
        push_note(ctx, &mut report.notes, format_args!(
            "{merged} merged component(s) retain occurrence-scoped model entities, native records, and source bytes"
        ))?;
    }
    push_note(ctx, &mut report.notes, format_args!(
        "merged {merged} external occurrence(s) from the f3z archive"
    ))?;
    merge::make_sibling_ordinals_unique(ctx, &mut ir.model.occurrences)?;
    append_losses(ctx, &mut report.losses, outer.losses)?;
    finalize_result(ctx, ir, source, report, fidelity)
}

fn finalize_result(
    ctx: &DecodeContext<'_>,
    mut ir: cadmpeg_ir::CadIr,
    mut source: cadmpeg_ir::SourceMeta,
    body: DecodeBody,
    source_fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<Decoded, CodecError> {
    ir.finalize();
    let hash = crate::decode::document_local_sha256_with_source(&ir, &source)?;
    ctx.charge_collection_items(1, "record F3Z document digest")?;
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
