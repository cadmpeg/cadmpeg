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
    target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput, ExportBody, PatchConsumption,
    WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;

use super::write;
use crate::native::DocumentFacts;

/// What resolving a [`cadmpeg_ir::codec::write::target::TargetRequest`] against the source decided.
///
/// This writer has one capability. It patches the retained
/// `Document.xml` and regenerates none, so the only dialect it can deliver is
/// the one the retained document already declares. Every other resolution is a
/// refusal, not a degraded write: there is no synthesis path to degrade to.
/// Only [`resolve`] builds one: its fields are private, so a `Resolution` in
/// hand proves that the retained document graph delivers the options it carries.
/// [`write_seekable`] takes that proof instead of raw options, which is why it
/// needs no target gate of its own.
#[derive(Debug)]
pub(super) struct Resolution<'a> {
    ir: &'a CadIr,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
    schema_version: String,
    target: DialectId,
}

impl<'a> Resolution<'a> {
    pub(super) const fn ir(&self) -> &'a CadIr {
        self.ir
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

const BASELINE_UNAVAILABLE: &str =
    "the retained FCStd document graph is unavailable, and this writer regenerates no \
     Document.xml, so the target cannot be written";
const TRANSCODE_UNAVAILABLE: &str =
    "the resolved target displaces the retained FCStd source dialect, and this writer \
     regenerates no Document.xml, so the target cannot be written";

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog. It is otherwise
/// the replay law's compare: the retained document is written back exactly when
/// the retained graph can deliver `id`, and any other id is a transcode this
/// writer cannot perform, refused by name with the catalog.
///
/// `Inherit` asks for preservation instead. This writer repacks the retained
/// entry set and patches `Document.xml` inside it, which reproduces whatever
/// schema the source declared — schema 2 and schema 3 included, neither of which
/// is a synthesis target. Where the retained document graph cannot carry the
/// source's dialect, `Inherit` refuses, naming that dialect and the catalog.
/// There is no fall-through to the catalog default: a same-format conversion
/// never silently changes what the file is. `fcstd:schema-2` is the canonical
/// off-catalog preservation case. An explicit target can name a catalog row,
/// but no request can override the retained graph's deliverability.
///
/// An `FCStd` source that records no dialect is refused too: there is nothing to
/// preserve, and no identity to default to. A source of another format is also
/// refused because this catalog intentionally has no cross-format default: the
/// writer cannot synthesize the retained `FCStd` graph that its only row needs.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let resolution = resolve(input.ir, resolved)?;
    admit_unedited(input.ir)?;
    finish(&resolution)
}

/// Native `fcstd` arenas this writer handles itself: property values patch
/// `Document.xml`, entry payloads are repacked, and the writer refuses its own
/// unsupported object, extension and unreadable-entry edits.
const CARRIED_NATIVE_ARENAS: &[&str] = &[
    "entries",
    "extensions",
    "objects",
    "properties",
    "unreadable_entries",
];

/// The `document_local_sha256` an `FCStd` write compares.
///
/// Covers every neutral arena, the source metadata, and every native record
/// except [`CARRIED_NATIVE_ARENAS`]. Outside those arenas the writer reads only
/// the source dialect and the `document` record, to select and name the
/// retained graph, so a change to anything this digest covers does not reach
/// the output.
pub(crate) fn document_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    operation: &'static str,
) -> Result<String, CodecError> {
    // FCStd retains no whole-file source image, so no unknown record is
    // excluded as one.
    cadmpeg_ir::hash::document_local_sha256_without_carried(
        ctx,
        ir,
        ir.source.as_ref(),
        "fcstd",
        "",
        CARRIED_NATIVE_ARENAS,
        operation,
    )
}

/// Refuse a document edited since decode outside [`CARRIED_NATIVE_ARENAS`].
///
/// The writer patches retained records and regenerates no `Document.xml`, so
/// a neutral edit has no serializer here and would otherwise be dropped.
fn admit_unedited(ir: &CadIr) -> Result<(), CodecError> {
    let Some(expected) = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
    else {
        return Err(CodecError::NotImplemented(format!(
            "FCStd source carries no `{DOCUMENT_LOCAL_DIGEST_ATTRIBUTE}` baseline, so neutral \
             edits cannot be excluded and source-less graph regeneration is required"
        )));
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let current = document_local_sha256(&ctx, ir, "FCStd write edit digest")?;
    ctx.finish_session()?;
    if current != *expected {
        return Err(CodecError::NotImplemented(
            "neutral or uncarried native edits since decode require source-less FCStd graph \
             regeneration"
                .into(),
        ));
    }
    Ok(())
}

/// Write the resolved export. The sealed encoder stamps the target identity
/// and the fidelity resolution; this writer patches the retained document and
/// consumes no sidecar. [`admit_unedited`] has proved that nothing outside the
/// carried native arenas changed since decode, so the retained graph carries
/// every model arena.
fn finish(resolution: &Resolution<'_>) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: WritePath::Patched {
            consumption: PatchConsumption::Independent(Consumption::NotConsumed),
        },
        coverage: ArenaCoverage::Complete,
        losses: Vec::new(),
        notes: outcome.notes,
    })
}

/// Decide what to write, from the request and the source.
fn resolve<'a>(ir: &'a CadIr, resolved: &ResolvedWrite<'_>) -> Result<Resolution<'a>, CodecError> {
    if !resolved.source_preservation_eligible() {
        return Err(resolved.unavailable(TRANSCODE_UNAVAILABLE));
    }
    // Target resolution already classified the source declaration. The
    // accepted Resolution threads that witness to the byte writer; the byte
    // writer does not classify the native declaration again.
    retained_baseline(ir, resolved.target_id())
        .ok_or_else(|| resolved.unavailable(BASELINE_UNAVAILABLE))
}

/// The write options and dialect witnessed by the retained document graph.
///
/// The graph is the whole baseline: the writer never regenerates a
/// `Document.xml`, so preservation is possible exactly when the retained
/// document record is present. `target` is the dialect witness resolved from
/// the source declaration; this adapter does not derive a second identity from
/// the retained graph.
pub(super) fn retained_baseline<'a>(ir: &'a CadIr, target: &DialectId) -> Option<Resolution<'a>> {
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
        namespace,
        document: document.clone(),
        schema_version,
        target: target.clone(),
    })
}
