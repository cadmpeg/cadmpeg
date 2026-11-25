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
    carried_source_losses, edit_unverified,
    target::{ResolvedWrite, WrittenRow},
    ArenaCoverage, Consumption, EncodeInput, ExportBody, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::SourceFidelity;

use super::write;
use crate::native::DocumentFacts;

/// What resolving a [`cadmpeg_ir::codec::write::target::TargetRequest`] against the source decided.
///
/// This writer has one capability. It patches the retained
/// `Document.xml` and regenerates none, so the only dialect it can deliver is
/// the one the retained document already declares. A request for another row
/// is written as that dialect (§3.4, Refusals).
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
    /// The source dialect written in place of the requested row.
    displaced: Option<WrittenRow>,
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
const PATCH_ONLY: &str =
    "this writer patches the retained FCStd Document.xml and regenerates none, so it writes \
     the dialect the retained document declares";

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog. It is otherwise
/// the replay law's compare: the retained document is written back as `id`
/// when the retained graph delivers it, and as the source's own dialect when
/// `id` displaces it, with `target.request-displaced` naming both.
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
/// preserve, and the catalog declares no default. A source of another format is
/// also refused because this catalog intentionally has no cross-format default:
/// the writer cannot synthesize the retained `FCStd` graph that its only row
/// needs.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let resolution = resolve(input.ir, resolved)?;
    let baseline = admit_unedited(input.ir, input.fidelity)?;
    finish(&resolution, input.fidelity, baseline)
}

/// Native `fcstd` arenas this writer handles itself: property values patch
/// `Document.xml`, entry payloads are repacked, and the writer refuses its own
/// unsupported object and extension edits.
const CARRIED_NATIVE_ARENAS: &[&str] = &["entries", "extensions", "objects", "properties"];

/// The codec digest (§3.1, Rule B) under which a `Full` decode records
/// [`uncarried_digest`] and an `FCStd` write compares it.
pub(crate) const UNCARRIED_DIGEST: &str = "fcstd_uncarried_local_sha256";

/// The digest an `FCStd` write compares to find edits it cannot write.
///
/// Covers every neutral arena, the source metadata, and every native record
/// except [`CARRIED_NATIVE_ARENAS`]. Outside those arenas the writer reads only
/// the source dialect and the `document` record, to select and name the
/// retained graph, so a change to anything this digest covers does not reach
/// the output.
pub(crate) fn uncarried_digest(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Sha256Digest, CodecError> {
    // FCStd retains no whole-file source image, so no unknown record is
    // excluded as one.
    let digest = cadmpeg_ir::hash::document_local_sha256_without_carried(
        ctx,
        ir,
        ir.source.as_ref(),
        "fcstd",
        "",
        CARRIED_NATIVE_ARENAS,
    )?;
    Sha256Digest::try_from(digest).map_err(|_| {
        CodecError::Malformed("the FCStd uncarried digest is no SHA-256 digest".into())
    })
}

/// What [`admit_unedited`] could establish about edits since decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Baseline {
    /// The digest matches: nothing outside [`CARRIED_NATIVE_ARENAS`] changed.
    Unedited,
    /// The sidecar records no digest, so the document's edits are not known.
    Unrecorded,
}

/// Refuse a document edited since decode outside [`CARRIED_NATIVE_ARENAS`].
///
/// The writer patches retained records and regenerates no `Document.xml`, so
/// a neutral edit has no serializer here and would otherwise be dropped. A
/// document whose sidecar records no digest (no sidecar, or every decode
/// below `Full`) is admitted as [`Baseline::Unrecorded`].
fn admit_unedited(ir: &CadIr, fidelity: Option<&SourceFidelity>) -> Result<Baseline, CodecError> {
    let Some(expected) = fidelity.and_then(|fidelity| fidelity.codec_digests.get(UNCARRIED_DIGEST))
    else {
        return Ok(Baseline::Unrecorded);
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let current = uncarried_digest(&ctx, ir)?;
    ctx.finish_session()?;
    if current != *expected {
        return Err(CodecError::NotImplemented(
            "neutral or uncarried native edits since decode require source-less FCStd graph \
             regeneration"
                .into(),
        ));
    }
    Ok(Baseline::Unedited)
}

/// Write the resolved export. The sealed encoder stamps the target identity
/// and the fidelity resolution; this writer patches the retained document and
/// consumes no sidecar. The retained graph carries every model arena that
/// [`admit_unedited`] found unedited. Every write is a patch, so `untyped`
/// retained records in `fidelity` charge both carried-source codes, and a
/// [`Baseline::Unrecorded`] document charges `export:edit.unverified`.
fn finish(
    resolution: &Resolution<'_>,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
    baseline: Baseline,
) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    let write_path = WritePath::Patched {
        consumption: PatchConsumption::Independent(Consumption::NotConsumed),
    };
    let mut losses = carried_source_losses(fidelity, &write_path);
    if baseline == Baseline::Unrecorded {
        losses.push(edit_unverified(format!(
            "the FCStd decode sidecar records no `{UNCARRIED_DIGEST}` codec digest, so edits \
             since decode are not known; the file patches the retained Document.xml with the \
             carried native records and holds no other edit"
        )));
    }
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        losses,
        write_path,
        coverage: ArenaCoverage::Complete,
        notes: outcome.notes,
        written_row: resolution.displaced.clone(),
    })
}

/// Decide what to write, from the request and the source.
fn resolve<'a>(ir: &'a CadIr, resolved: &ResolvedWrite<'_>) -> Result<Resolution<'a>, CodecError> {
    // Target resolution already classified the source declaration. The
    // accepted Resolution threads that witness to the byte writer; the byte
    // writer does not classify the native declaration again.
    let displaced = resolved.displaced_source().map(|source| WrittenRow {
        target: source.clone(),
        reason: PATCH_ONLY.to_owned(),
    });
    let target = displaced
        .as_ref()
        .map_or_else(|| resolved.target_id(), |row| &row.target);
    let mut resolution =
        retained_baseline(ir, target).ok_or_else(|| resolved.unavailable(BASELINE_UNAVAILABLE))?;
    resolution.displaced = displaced;
    Ok(resolution)
}

/// The write options and dialect witnessed by the retained document graph.
///
/// The graph is the whole baseline: the writer never regenerates a
/// `Document.xml`, so preservation is possible exactly when the retained
/// document record and its `Document.xml` entry are present. A `Container`
/// decode retains no entry. `target` is the dialect witness resolved from
/// the source declaration; this adapter does not derive a second identity from
/// the retained graph.
pub(super) fn retained_baseline<'a>(ir: &'a CadIr, target: &DialectId) -> Option<Resolution<'a>> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let retains_document_xml = namespace.arenas().get("entries").is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry
                .field("name")
                .as_ref()
                .and_then(serde_json::Value::as_str)
                == Some("Document.xml")
        })
    });
    if !retains_document_xml {
        return None;
    }
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
        displaced: None,
    })
}
