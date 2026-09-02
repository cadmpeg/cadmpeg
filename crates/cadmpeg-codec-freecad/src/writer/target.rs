// SPDX-License-Identifier: Apache-2.0
//! Target resolution: what an `FCStd` write is allowed to be.
//!
//! The one gate on this codec's write law. [`Encoder::plan`] resolves here, and
//! [`super::write_seekable`] then carries out what a
//! [`Resolution`] settled without re-deciding any of it.
//!
//! [`Encoder::plan`]: cadmpeg_ir::codec::write::Encoder::plan

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{Consumption, EncodeInput, ExportBody, ResolvedWrite};
use cadmpeg_ir::document::CadIr;

use super::write;
use crate::dialect;
use crate::native::DocumentFacts;

/// What resolving a [`cadmpeg_ir::codec::write::TargetRequest`] against the source decided.
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
pub(crate) struct Resolution<'a> {
    ir: &'a CadIr,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
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
}

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
/// case, and an explicit `--to` is the escape — from the inherit refusal, not
/// from the deliverability one, which no request can talk this writer out of.
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
    finish(&resolution)
}

/// Write the resolved export. The sealed encoder stamps the target identity
/// and the fidelity resolution; this writer patches the retained document and
/// consumes no sidecar.
fn finish(resolution: &Resolution<'_>) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: cadmpeg_ir::WritePath::Patched,
        losses: Vec::new(),
        notes: outcome.notes,
        consumption: Consumption::NotConsumed,
    })
}

/// Decide what to write, from the request and the source.
pub(in crate::writer) fn resolve<'a>(
    ir: &'a CadIr,
    resolved: &ResolvedWrite<'_>,
) -> Result<Resolution<'a>, CodecError> {
    let target = resolved.target_id();
    // Deliverability, not preference. This writer patches the retained
    // `Document.xml` and regenerates none, so the resolved target is reachable
    // exactly when the retained graph already declares it — §8.1's "a
    // patch-only writer's row is reachable only from a retained source of that
    // flavor, and the plan refuses by name where it cannot deliver". The
    // refusal is typed and carries the catalog, like every other write refusal;
    // it used to surface as a bare message string from deep inside `write`.
    retained_baseline(ir, target).ok_or_else(|| {
        resolved.unavailable(
            "the retained FCStd document graph does not declare it, and this writer \
                 regenerates no Document.xml, so it cannot be written",
        )
    })
}

/// The write options that reproduce `source_dialect` from the retained document
/// graph, or `None` where that graph cannot carry it.
///
/// The graph is the whole baseline: the writer never regenerates a
/// `Document.xml`, so preservation is possible exactly when the retained
/// document record is present and its exact declaration classifies as the
/// source's own dialect. A `SchemaVersion` of `"04"` classifies as
/// `fcstd:unknown`, so it is preserved as residual identity rather than being
/// rewritten as `"4"`.
pub(in crate::writer) fn retained_baseline<'a>(
    ir: &'a CadIr,
    source_dialect: &DialectId,
) -> Option<Resolution<'a>> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let dialect = dialect::FcstdDialect::from_schema_version(&document.schema_version);
    (dialect.id() == *source_dialect).then(|| Resolution {
        ir,
        namespace,
        document: document.clone(),
    })
}
