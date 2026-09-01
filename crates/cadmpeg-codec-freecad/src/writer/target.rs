// SPDX-License-Identifier: Apache-2.0
//! Target resolution: what an `FCStd` write is allowed to be.
//!
//! The one gate on this codec's write law. [`Encoder::plan`] resolves here, and
//! [`super::write_seekable`] then carries out what a
//! [`Resolution`] settled without re-deciding any of it.
//!
//! [`Encoder::plan`]: cadmpeg_ir::codec::Encoder::plan

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{ExportReport, FidelityResolution};

use super::write;
use crate::dialect;
use crate::native::DocumentFacts;

/// What resolving a [`TargetRequest`] against the source decided.
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
    dialect: crate::dialect::FcstdDialect,
    ir: &'a CadIr,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
}

impl<'a> Resolution<'a> {
    pub(super) const fn dialect(&self) -> crate::dialect::FcstdDialect {
        self.dialect
    }

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
    request: TargetRequest<'_>,
) -> Result<ExportPlan, CodecError> {
    let resolution = resolve(input.ir, request)?;
    finish(input, &resolution)
}

/// Write the resolved export and state what the fidelity sidecar did.
fn finish(input: EncodeInput<'_>, resolution: &Resolution<'_>) -> Result<ExportPlan, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    // A plan constructs its report once, after every report input is final.
    let report = ExportReport::native(
        outcome.target,
        outcome.census,
        if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        },
        cadmpeg_ir::WritePath::Patched,
        Vec::new(),
        outcome.notes,
    );
    Ok(ExportPlan::buffered(report, bytes))
}

/// Decide what to write, from the request and the source.
pub(in crate::writer) fn resolve<'a>(
    ir: &'a CadIr,
    request: TargetRequest<'_>,
) -> Result<Resolution<'a>, CodecError> {
    // This writer has no synthesize fallback, so it flattens the request locally.
    let resolved =
        cadmpeg_ir::codec::resolve_write_request(ir, request, dialect::FORMAT, dialect::TARGETS)?;
    let target = resolved.dialect().clone();
    // Deliverability, not preference. This writer patches the retained
    // `Document.xml` and regenerates none, so the resolved target is reachable
    // exactly when the retained graph already declares it — §8.1's "a
    // patch-only writer's row is reachable only from a retained source of that
    // flavor, and the plan refuses by name where it cannot deliver". The
    // refusal is typed and carries the catalog, like every other write refusal;
    // it used to surface as a bare message string from deep inside `write`.
    retained_baseline(ir, &target).ok_or_else(|| {
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
fn retained_baseline<'a>(ir: &'a CadIr, source_dialect: &DialectId) -> Option<Resolution<'a>> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let dialect = dialect::FcstdDialect::from_schema_version(&document.schema_version);
    (dialect != dialect::FcstdDialect::Unknown && dialect.id() == *source_dialect).then(|| {
        Resolution {
            dialect,
            ir,
            namespace,
            document: document.clone(),
        }
    })
}
