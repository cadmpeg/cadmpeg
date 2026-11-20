// SPDX-License-Identifier: Apache-2.0
//! Writing a `SolidCraft` design.
//!
//! An unedited design is written back byte for byte from the retained source
//! image. The document digest recorded at decode is the edit oracle: while the
//! document still digests to it, nothing outside the image changed and the
//! image is the whole answer. A document that changed, or one with no image,
//! needs the semantic writer, which this module does not have yet: those are
//! refused by name instead of written wrong.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::report::export::{CensusBasis, EntityCensus};

use crate::dialect::FORMAT;

mod builder;
mod semantic;

/// The identity of the retained source image record.
pub(crate) fn source_image_id() -> UnknownId {
    UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("solidcraft", "native", "source-image"),
        cadmpeg_ir::identity_key!("0"),
    )
}

/// The `document_local_sha256` a write compares: every neutral arena, the
/// source metadata, and every native record except the source image.
pub(crate) fn document_local_sha256(ir: &CadIr) -> Result<String, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let digest = cadmpeg_ir::hash::document_local_sha256(
        &ctx,
        ir,
        ir.source.as_ref(),
        FORMAT,
        source_image_id().as_str(),
    )?;
    ctx.finish_session()?;
    Ok(digest)
}

/// Plan the export the resolved request names.
///
/// A request that preserves the source (an inherited target, or the source's
/// own dialect) replays or patches the retained image. Any other request, or a
/// source-less model, is generated from the neutral model by [`builder`].
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let image = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(source_image_id().as_str()))
        .and_then(|record| record.data());
    if target.entry().is_none() || target.preserves_source() {
        match image {
            Some(image) => return preserve(input, image),
            None if target.entry().is_none() => {
                return Err(target.unavailable(
                    "its retained source image is unavailable, and the dialect is not one this                      writer generates",
                ))
            }
            None => {}
        }
    }
    let encoding = if *target.target_id() == crate::dialect::SOLIDCRAFT_JSON_V57 {
        builder::Encoding::Json
    } else {
        builder::Encoding::Container
    };
    let consumption = match (image, target.displacement_message()) {
        (None, _) if target.source_preservation_eligible() => Consumption::Degraded {
            reason: "the retained SolidCraft source image is unavailable".into(),
        },
        _ => Consumption::NotConsumed,
    };
    let mut body = builder::write(input, encoding, consumption)?;
    body.notes.extend(target.displacement_message());
    Ok(body)
}

/// Replay the image of an unedited design, or patch an edited one.
fn preserve(input: EncodeInput<'_>, image: &[u8]) -> Result<ExportBody, CodecError> {
    let baseline = input
        .ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
        .ok_or_else(|| {
            CodecError::NotImplemented("the document carries no SolidCraft write baseline".into())
        })?;
    if document_local_sha256(input.ir)? != *baseline {
        return semantic::write(input, image);
    }
    Ok(ExportBody {
        bytes: image.to_vec(),
        census: EntityCensus {
            basis: CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path: WritePath::VerbatimReplay,
        coverage: ArenaCoverage::Complete,
        losses: Vec::new(),
        notes: vec!["the retained SolidCraft source image was replayed unchanged".into()],
    })
}
