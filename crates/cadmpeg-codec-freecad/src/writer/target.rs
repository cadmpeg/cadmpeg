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

use super::{write_seekable, PatchError};
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
        if let cadmpeg_ir::codec::write::ReducedStorage::Normalized { loss } = row.storage {
            debug_assert!(
                loss.starts_with("fcstd/"),
                "reduced row {} charges a namespaced normalization loss",
                row.path
            );
        }
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
            let matches = cadmpeg_ir::hash::retained_source_matches(
                &allocation_context,
                input.ir,
                resolution.fidelity(),
                "fcstd",
                crate::fidelity::SOURCE_IMAGE,
                |ctx, root| {
                    let fresh =
                        cadmpeg_ir::Codec::decode_with_context(&crate::FcstdCodec, ctx, root)
                            .ok()?;
                    cadmpeg_ir::hash::document_local_sha256(
                        ctx,
                        fresh.ir(),
                        fresh.ir().source.as_ref(),
                        "fcstd",
                        crate::fidelity::SOURCE_IMAGE,
                    )
                    .ok()
                },
            );
            let patch = match matches {
                Ok(true) => Some(allocation_context.run_part(|| {
                    match finish(&allocation_context, &resolution) {
                        Ok(body) => Ok(Ok(body)),
                        Err(PatchError::Declined(reason)) => Ok(Err(reason)),
                        Err(PatchError::Codec(error)) => Err(error),
                    }
                })?),
                Ok(false) => {
                    reason =
                        Some("the current document no longer matches its retained image".into());
                    None
                }
                Err(
                    error @ CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
                        scope: cadmpeg_core::decode::LimitScope::Part,
                        ..
                    }),
                ) => {
                    reason = Some(error.to_string());
                    None
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                // Reading the retained source only selects the write path.
                Err(error) => {
                    reason = Some(error.to_string());
                    None
                }
            };
            match patch {
                Some(Ok(Ok(body))) => return Ok(body),
                Some(Ok(Err(declined))) => reason = Some(declined),
                Some(Err(
                    error @ CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
                        scope: cadmpeg_core::decode::LimitScope::Part,
                        ..
                    }),
                )) => reason = Some(error.to_string()),
                Some(Err(error @ CodecError::ResourceLimit(_))) => return Err(error),
                Some(Err(error)) => reason = Some(error.to_string()),
                None => {}
            }
        } else {
            reason = Some("the retained typed document layout is unavailable".into());
        }
    }
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

/// Write the resolved retained layout using source images from fidelity.
/// The shared source comparison selects layout only.
fn finish(ctx: &DecodeContext<'_>, resolution: &Resolution<'_>) -> Result<ExportBody, PatchError> {
    let mut output = std::io::Cursor::new(Vec::new());
    let outcome = write_seekable(&mut output, resolution, ctx)?;
    let bytes = output.into_inner();
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
