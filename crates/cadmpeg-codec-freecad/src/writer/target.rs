// SPDX-License-Identifier: Apache-2.0
//! Target resolution: what an `FCStd` write is allowed to be.
//!
//! The one gate on this codec's write law. [`Encoder::plan`] resolves here, and
//! [`super::write_seekable`] then carries out what a
//! [`Resolution`] settled without re-deciding any of it.
//!
//! [`Encoder::plan`]: cadmpeg_ir::codec::write::Encoder::plan

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, EncodeInput, ExportBody, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;

pub(super) mod projection;

use super::write;
use crate::native::DocumentFacts;

/// What resolving a [`cadmpeg_ir::codec::write::target::TargetRequest`] against the source decided.
///
/// The patch path preserves admitted source layout. Synthesis authors all
/// content from the document when that path is unavailable. Private fields
/// prove that the retained typed document supplies the selected vocabulary.
/// [`write_seekable`] takes that proof instead of raw options, which is why it
/// needs no target gate of its own.
#[derive(Debug)]
pub(super) struct Resolution<'a> {
    ir: &'a CadIr,
    fidelity: &'a cadmpeg_ir::SourceFidelity,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
    schema_version: String,
    target: DialectId,
}

impl<'a> Resolution<'a> {
    pub(super) const fn ir(&self) -> &'a CadIr {
        self.ir
    }

    pub(super) const fn fidelity(&self) -> &'a cadmpeg_ir::SourceFidelity {
        self.fidelity
    }

    pub(super) const fn namespace(&self) -> &'a cadmpeg_ir::native::NativeNamespace {
        self.namespace
    }

    pub(super) const fn document(&self) -> &DocumentFacts {
        &self.document
    }

    pub(super) fn schema_version(&self) -> &str {
        &self.schema_version
    }

    pub(super) const fn target(&self) -> &DialectId {
        &self.target
    }
}

/// Patch an admitted unchanged source layout, otherwise author the current
/// document. A patch can decline an edit; synthesis does not consume bytes.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    debug_assert!(
        super::CAPACITY_FIELDS.is_empty(),
        "FreeCAD declares no capacity rows"
    );
    for row in super::REDUCED_FIELDS {
        debug_assert!(
            !row.path.is_empty() && row.path.starts_with("model."),
            "reduced row {} names a neutral path",
            row.path
        );
        debug_assert!(
            matches!(
                row.storage,
                cadmpeg_ir::codec::write::ReducedStorage::Normalized { loss }
                if loss.starts_with("fcstd/")
            ),
            "reduced row {} charges a namespaced normalization loss",
            row.path
        );
    }
    let allocation_arena = cadmpeg_core::decode::DecodeArena::new();
    let allocation_policy = cadmpeg_core::decode::DecodePolicy {
        limits: input.limits,
        ..cadmpeg_core::decode::DecodePolicy::default()
    };
    let allocation_context = DecodeContext::for_encode(&allocation_arena, &allocation_policy);
    // Synthesis visits every input record, so the caller's entity ceiling
    // applies to the input record count before any building (#217 §5.0).
    allocation_context.charge_entities(
        cadmpeg_core::decode::u64_from_index(input.ir.model.entity_count()),
        "write FCStd records",
    )?;
    let mut reason = None;
    if let Some(fidelity) = input
        .fidelity
        .filter(|_| resolved.source_preservation_eligible() && input.can_reuse_source())
    {
        if let Some(resolution) = retained_baseline(input.ir, fidelity, resolved.target_id()) {
            let patch = admit_unedited(
                &allocation_context,
                input.ir,
                resolution.fidelity(),
                input.limits,
            )
            .and_then(|()| finish(&resolution));
            match patch {
                Ok(body) => return Ok(body),
                Err(error @ CodecError::UnsupportedContent(_)) => reason = Some(error.to_string()),
                Err(error) => return Err(error),
            }
        } else {
            reason = Some("the retained typed document layout is unavailable".into());
        }
    }
    // Synthesis authors the recorded schema grammars only. A preserved
    // off-catalog dialect has no synthesis vocabulary: it patches from its
    // retained image or refuses as an unsupported target (#238 §5.0).
    let synthesizable = [
        crate::dialect::FcstdDialect::Schema2,
        crate::dialect::FcstdDialect::Schema3,
        crate::dialect::FcstdDialect::Schema4,
    ];
    if !synthesizable
        .iter()
        .any(|known| known.id().as_str() == resolved.target_id().as_str())
    {
        return Err(CodecError::UnsupportedTarget(Box::new(
            cadmpeg_core::target::TargetRefusal::unknown_explicit(
                resolved.target_id().as_str(),
                cadmpeg_core::target::TargetCatalog::new(crate::dialect::TARGETS, Some(0)),
            ),
        )));
    }
    let synthesis = resolved.for_synthesis()?;
    let resolved = synthesis.resolution();
    let schema = if *resolved.target_id() == crate::dialect::FcstdDialect::Schema2.id() {
        "2"
    } else if *resolved.target_id() == crate::dialect::FcstdDialect::Schema3.id() {
        "3"
    } else {
        debug_assert_eq!(
            *resolved.target_id(),
            crate::dialect::FcstdDialect::Schema4.id(),
            "FreeCAD catalog rows match schema grammars"
        );
        "4"
    };
    let mut body = super::synthesis::write(&allocation_context, input.ir, schema)?;
    if let Some(message) = resolved.displacement_message() {
        body.losses
            .push(crate::loss::FreecadLossCode::SourceDialectDisplaced.note(message));
    }
    if let Some(reason) = reason {
        body.synthesis_cause =
            Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason });
    }
    Ok(body)
}

/// Admit the retained layout only for an unedited document.
///
/// Patch preserves layout and identities; it never supplies content. Any
/// difference between the current document and a fresh decode of its
/// retained image — neutral or native — declines to synthesis with a
/// `PatchDeclined` reason (#238). A missing image declines the same way;
/// it never fails the write.
fn admit_unedited(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    // The gate admits the retained layout only when the retained image still
    // digests to the source baseline and decodes to the current document.
    // A missing image declines to synthesis; it never fails the write.
    let Some(image) = fidelity
        .retained_record(crate::fidelity::SOURCE_IMAGE)
        .and_then(|record| record.data())
    else {
        return Err(CodecError::UnsupportedContent(
            "the retained FCStd source image is unavailable".into(),
        ));
    };
    let current = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        ir,
        ir.source.as_ref(),
        "fcstd",
        crate::fidelity::SOURCE_IMAGE,
    )?;
    let replays = cadmpeg_ir::hash::retained_image_replays(
        image,
        &fidelity.decode_digest,
        current.as_str(),
        ir.scope,
        limits,
        |_, _| {
            // The writer plans from Full-depth documents (REQUEST), so the
            // replay decodes the image at Full even when the document scope
            // is narrower; the scope-derived depth would drop held records.
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy {
                limits,
                ..cadmpeg_core::decode::DecodePolicy::default()
            };
            let (fresh_ctx, fresh_root) = cadmpeg_core::decode::DecodeContext::from_root_bytes_at(
                image,
                &arena,
                &policy,
                cadmpeg_core::decode::DecodeDepth::Full,
            )
            .map_err(|_| {
                CodecError::UnsupportedContent(
                    "the retained FCStd source image refused to decode".into(),
                )
            })?;
            let fresh =
                cadmpeg_ir::Codec::decode_with_context(&crate::FcstdCodec, &fresh_ctx, fresh_root)
                    .map_err(|failure| match failure {
                        cadmpeg_ir::codec::DecodeFailure::Codec(error) => error,
                        _ => CodecError::UnsupportedContent(
                            "the retained FCStd source image refused to decode".into(),
                        ),
                    })?;
            let hash_arena = cadmpeg_core::decode::DecodeArena::new();
            let hash_policy = cadmpeg_core::decode::DecodePolicy::default();
            let (hash_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &hash_arena,
                &hash_policy,
            )?;
            let fresh_hash = cadmpeg_ir::hash::document_local_sha256(
                &hash_ctx,
                fresh.ir(),
                fresh.ir().source.as_ref(),
                "fcstd",
                crate::fidelity::SOURCE_IMAGE,
            )?;
            hash_ctx.finish_session()?;
            Ok(fresh_hash)
        },
    );
    if !replays {
        return Err(CodecError::UnsupportedContent(
            "the current document no longer matches its retained image".into(),
        ));
    }
    Ok(())
}

/// Write the resolved retained layout using source images from fidelity.
/// [`admit_unedited`] admits only an unedited document; the fingerprint
/// selects layout only.
fn finish(resolution: &Resolution<'_>) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
        coverage: ArenaCoverage::Complete,
        native_dispositions: cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
            "fcstd",
            crate::native::ARENAS,
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        ),
        synthesis_cause: None,
        losses: if outcome.geometry_empty {
            vec![cadmpeg_ir::codec::write::ExportLossCode::OutputEmpty
                .note("the emitted FCStd document has no objects or geometry")]
        } else {
            Vec::new()
        },
        notes: outcome.notes,
    })
}

/// The write options and dialect witnessed by the retained document graph.
///
/// Preservation requires a retained document record. `target` is the dialect witness resolved from
/// the source declaration; this adapter does not derive a second identity from
/// the retained graph.
pub(super) fn retained_baseline<'a>(
    ir: &'a CadIr,
    fidelity: &'a cadmpeg_ir::SourceFidelity,
    target: &DialectId,
) -> Option<Resolution<'a>> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let schema_version = ir
        .source
        .as_ref()?
        .dialect()?
        .declared()
        .get(crate::dialect::DECLARED_SCHEMA_VERSION)?
        .clone();
    Some(Resolution {
        ir,
        fidelity,
        namespace,
        document: document.clone(),
        schema_version,
        target: target.clone(),
    })
}

#[cfg(test)]
mod tests;
