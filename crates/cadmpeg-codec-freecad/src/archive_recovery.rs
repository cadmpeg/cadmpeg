// SPDX-License-Identifier: Apache-2.0
//! Delivery of a genuinely given-up archive before any document records escape.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::source_fidelity::parts::PartOutput;
use cadmpeg_ir::{CadIr, ContainerSummary, SourceMeta};

pub(crate) fn decoded(
    ctx: &DecodeContext<'_>,
    root: cadmpeg_core::decode::View<'_>,
    summary: ContainerSummary,
    output: PartOutput,
    document_found: bool,
    archive_given_up: bool,
) -> Result<Decoded, CodecError> {
    let source = match summary.dialects().cloned() {
        Some(dialects) => SourceMeta::classified(dialects, BTreeMap::new()),
        None => SourceMeta::unclassified(cadmpeg_ir::CodecFormat::Fcstd, BTreeMap::new()),
    };
    let (model, native, mut fidelity, mut losses) = output.into_parts();
    let mut ir = CadIr::decoded(source);
    ir.model = model;
    ir.native = native;
    // A located document class remains held even when its part, or an
    // enclosing archive, gave up.
    let scope = if document_found {
        ctx.depth().required_scope()
    } else {
        cadmpeg_ir::ContentScope::container()
    };
    ir.set_scope(scope).map_err(CodecError::malformed)?;
    losses.extend(summary.losses);
    if !archive_given_up {
        ctx.retain_bytes(
            u64_from_index(root.window().len()),
            "retain FCStd source image",
        )?;
        crate::fidelity::retain(
            &mut fidelity,
            crate::fidelity::SOURCE_IMAGE,
            "",
            u64_from_index(root.start()),
            root.window(),
            cadmpeg_ir::ArenaClasses::new(),
        )?;
    }
    let digest = crate::writer::target::document_local_sha256(ctx, &ir)?;
    fidelity.metadata.insert(
        cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY.into(),
        digest,
    );
    Ok(Decoded {
        body: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: false,
            },
            coverage: Default::default(),
            losses,
            notes: summary.notes,
        },
        ir,
        source_fidelity: fidelity,
    })
}
