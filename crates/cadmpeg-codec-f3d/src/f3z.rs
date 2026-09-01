// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds one manifest-selected root and one member per document.
//! [`decode`] classifies every member, decodes the model root, and delegates
//! occurrence-scoped graph composition to [`merge`].

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

mod archive;
mod merge;

/// Inspects every document member under the F3Z archive identity.
pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ContainerSummary, CodecError> {
    let (model_root, _) = archive::model_root(scan)?;
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
    let notes = vec![format!(
        "f3z archive: {member_count} document member(s); model root {model_root}"
    )];
    Ok(ContainerSummary::classified(
        classified.layers,
        "zip",
        scan.entries.clone(),
        classified.losses,
        notes,
    ))
}

/// Decodes a scanned `.f3z` archive into one occurrence-scoped document.
pub fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<DecodeResult, CodecError> {
    let (model_root, omitted_drawing_root) = archive::model_root(scan)?;
    let outer = archive::classify_members(ctx, scan)?;
    let root_scan = outer.member_scan(&model_root)?;
    let (mut ir, mut report, mut fidelity) =
        crate::decode::decode_archive_member(ctx, root_scan)?.into_parts();
    fidelity
        .retained_records
        .retain(|record| record.id != crate::ids::FILE_SOURCE_IMAGE_ID);
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(scan)]);
    if let Some(drawing_root) = omitted_drawing_root {
        report
            .losses
            .push(F3dLossCode::DrawingDocumentOmitted.note(format!(
                "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
            )));
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
        return finalize_result(ir, classify_outer_report(report, outer), fidelity);
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
        fidelity
            .retained_records
            .retain(|record| record.id != crate::ids::FILE_SOURCE_IMAGE_ID);
        report.notes.push(format!(
            "{merged} merged component(s) retain occurrence-scoped model entities and native records; member source streams remain archive-local"
        ));
    }
    report.notes.push(format!(
        "merged {merged} external occurrence(s) from the f3z archive"
    ));
    merge::make_sibling_ordinals_unique(&mut ir.model.occurrences);
    finalize_result(ir, classify_outer_report(report, outer), fidelity)
}

fn classify_outer_report(
    mut report: cadmpeg_ir::DecodeReport,
    outer: archive::ArchiveSession<'_>,
) -> cadmpeg_ir::DecodeReport {
    report.losses.extend(outer.losses);
    cadmpeg_ir::DecodeReport::classified(
        outer.layers,
        report.transfer(),
        report.coverage,
        report.losses,
        report.notes,
        report.transfer_ledger,
    )
}

fn finalize_result(
    mut ir: cadmpeg_ir::CadIr,
    report: cadmpeg_ir::DecodeReport,
    fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<DecodeResult, CodecError> {
    if let Some(source) = ir.source.as_mut() {
        *source = cadmpeg_ir::SourceMeta::unclassified(
            report.format(),
            std::mem::take(&mut source.attributes),
        );
    }
    let mut result = DecodeResult::new(ir, report, fidelity)?;
    let hash = crate::decode::document_local_sha256(result.ir());
    if let Some(source) = &mut result.ir_mut().source {
        source.attributes.insert(
            cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(),
            hash,
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
