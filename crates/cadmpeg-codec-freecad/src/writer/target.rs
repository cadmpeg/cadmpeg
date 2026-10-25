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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits: input.limits,
        ..cadmpeg_core::decode::DecodePolicy::default()
    };
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let body = plan_with_context(&ctx, input, resolved)?;
    ctx.finish_session()?;
    Ok(body)
}

fn plan_with_context(
    ctx: &DecodeContext<'_>,
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let mut reason = None;
    if let Some(fidelity) = input
        .fidelity
        .filter(|_| resolved.source_preservation_eligible() && input.can_reuse_source())
    {
        if let Some(resolution) = retained_baseline(input.ir, fidelity, resolved.target_id()) {
            let patch = admit_unedited(ctx, input.ir, resolution.fidelity())
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
    let mut body = super::synthesis::write(ctx, input.ir, schema)?;
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

/// Native `fcstd` arenas this writer handles itself: property values patch
/// `Document.xml`, entry payloads are repacked, and the writer refuses its own
/// unsupported object, extension and unreadable-entry edits.
const CARRIED_NATIVE_ARENAS: &[&str] = &["entries", "extensions", "objects", "properties"];

/// Fingerprint of typed native arenas whose edits this layout optimization
/// does not carry. It may decline layout reuse; neutral agreement is checked
/// independently by projecting the current typed graph.
pub(crate) fn document_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<String, CodecError> {
    let records = ir
        .native
        .namespace("fcstd")
        .map(|namespace| {
            namespace
                .arenas()
                .iter()
                .filter(|(arena, _)| !CARRIED_NATIVE_ARENAS.contains(&arena.as_str()))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    cadmpeg_ir::hash::canonical_json_sha256(ctx, &records).map_err(Into::into)
}

/// Decline the retained-layout optimization after an uncarried edit.
///
/// Such an edit is authored by the synthesized path instead.
fn admit_unedited(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    fidelity: &cadmpeg_ir::SourceFidelity,
) -> Result<(), CodecError> {
    if let Some(namespace) = ir.native.namespace("fcstd") {
        if namespace
            .arena_as::<crate::native::ObjectRecord>("objects")?
            .iter()
            .any(|object| {
                crate::design::is_design_kind(&object.type_name)
                    && !crate::design::is_design_object(&object.type_name)
            })
        {
            return Err(CodecError::UnsupportedContent(
                "unknown construction runtime types require typed native synthesis".into(),
            ));
        }
    }

    let expected = fidelity
        .metadata
        .get(cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY)
        .ok_or_else(|| {
            CodecError::UnsupportedContent(
                "the retained typed-layout fingerprint is unavailable".into(),
            )
        })?;
    let current = document_local_sha256(ctx, ir)?;
    if current != *expected {
        return Err(CodecError::UnsupportedContent(
            "current uncarried typed records require FCStd graph regeneration".into(),
        ));
    }
    projection::agrees(ctx, ir).map_err(|error| match error {
        CodecError::Malformed(_) | CodecError::Truncated { .. } => CodecError::UnsupportedContent(
            format!("current typed projection is unavailable: {error}"),
        ),
        error => error,
    })?;
    let findings = crate::validate_native(ctx, ir)?;
    if !findings.is_empty() {
        return Err(CodecError::UnsupportedContent(
            "the retained layout no longer agrees with its typed native records".into(),
        ));
    }
    Ok(())
}

/// Write the resolved retained layout using source images from fidelity.
/// [`admit_unedited`] separately proves that its current typed projection
/// agrees with every held neutral arena; the fingerprint selects layout only.
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
