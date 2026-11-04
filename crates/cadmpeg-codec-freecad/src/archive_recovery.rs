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
    found_design: bool,
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
    // #209 §1.1: a located document part proves its classes were found,
    // so model stays held when requested. A found but failed document part
    // stays in scope too; only located design carriers or a populated
    // design arena prove design beyond the document part. Design
    // additionally requires located design carriers: a cut decode may not
    // gain a class it never located (#224 ruling 1). With no located
    // document part, no class is held. A populated neutral arena always
    // holds its class.
    let required = ctx.depth().required_scope();
    let model = required.model
        && (ir
            .model
            .populated_arena(cadmpeg_ir::ArenaClass::Model)
            .is_some()
            || document_found);
    let design = required.design
        && (ir
            .model
            .populated_arena(cadmpeg_ir::ArenaClass::Design)
            .is_some()
            || found_design
            || document_found);
    ir.set_scope(cadmpeg_ir::ContentScope { model, design })
        .map_err(CodecError::malformed)?;
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
    // The decode wrapper stamps the source digest from its root view.
    Ok(Decoded {
        body: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer {
                scope: ir.scope,
                geometry_transferred: false,
            },
            coverage: cadmpeg_ir::report::decode::Coverage::default(),
            losses,
            notes: summary.notes,
        },
        ir,
        source_fidelity: fidelity,
    })
}
