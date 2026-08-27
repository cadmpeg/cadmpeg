// SPDX-License-Identifier: Apache-2.0
//! Target resolution: what an `FCStd` write is allowed to be (design §8.2).
//!
//! The one gate on this codec's write law. Every door into the writer —
//! [`Encoder::plan`] and [`crate::FcstdCodec::encode_with_options`] — resolves
//! here first, and [`super::write_seekable`] then carries out what a
//! [`Resolution`] settled without re-deciding any of it.
//!
//! [`Encoder::plan`]: cadmpeg_ir::codec::Encoder::plan

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{unsupported_target, EncodeInput, ExportPlan, TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::FidelityResolution;

use super::write;
use crate::dialect;
use crate::loss::FreecadLossCode;
use crate::native::DocumentFacts;
use crate::FcstdWriteOptions;

/// What resolving a [`TargetRequest`] against the source decided (design §8.2).
///
/// One field, because this writer has one capability. It patches the retained
/// `Document.xml` and regenerates none, so the only dialect it can deliver is
/// the one the retained document already declares. Every other resolution is a
/// refusal, not a degraded write: there is no synthesis path to degrade to.
/// Only [`resolve`] builds one: the field is private, so a `Resolution` in hand
/// is a proof that the retained document graph delivers the options it carries.
/// [`write_seekable`] takes that proof instead of raw options, which is why it
/// needs no target gate of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// The persistence band to write. Always the one the retained document
    /// graph carries.
    options: FcstdWriteOptions,
    displaced: Option<DialectId>,
}

impl Resolution {
    /// The write options the retained graph delivers.
    pub(crate) fn options(self) -> FcstdWriteOptions {
        self.options
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
/// preserve, and no identity to default to. The catalog default supplies the
/// target only when there is nothing to inherit at all — no source, or one of
/// another format.
pub(crate) fn plan<'a>(
    input: EncodeInput<'a>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan<'a>, CodecError> {
    let resolution = resolve(input.ir, request)?;
    finish(input, resolution)
}

/// Plan the write that [`crate::FcstdCodec::encode_with_options`] names.
///
/// The options name a persistence band, so they name a dialect, and that dialect
/// goes through the one resolution gate like every other request. Two halves are
/// checked, because a dialect id carries only one of them: [`resolve`] answers
/// for the `SchemaVersion`, and the comparison below answers for the
/// `FileVersion`, which no id can state. A caller that asks for a band the
/// retained graph does not carry is refused by name, with the catalog, before
/// any byte is written.
pub(crate) fn plan_options(
    input: EncodeInput<'_>,
    options: FcstdWriteOptions,
) -> Result<ExportPlan<'_>, CodecError> {
    let target = dialect::written_dialect(options);
    let resolution = resolve(input.ir, TargetRequest::Explicit(target.as_str()))?;
    if resolution.options != options {
        return Err(unsupported_target(
            dialect::FORMAT,
            target.as_str(),
            "the retained FCStd document graph declares another FileVersion, and this writer \
             regenerates no Document.xml, so it cannot be written",
            dialect::TARGETS,
        ));
    }
    finish(input, resolution)
}

/// Write the resolved export and state what the fidelity sidecar did.
fn finish(input: EncodeInput<'_>, resolution: Resolution) -> Result<ExportPlan<'_>, CodecError> {
    let mut bytes = Vec::new();
    let displaced = resolution.displaced.clone();
    let mut report = write(input.ir, &mut bytes, resolution)?;
    // `write` takes no fidelity sidecar, so the report it returns states the
    // only resolution it can see. Whether the caller supplied one is known
    // here, and only here. There is no degraded arm: a write that would change
    // the source's dialect does not reach this point, because this writer
    // cannot perform one and `resolve` refuses it by name.
    report.fidelity = if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    if let Some(source) = displaced.as_ref() {
        let target = report
            .target
            .as_ref()
            .expect("FCStd writes name their target");
        report
            .losses
            .push(FreecadLossCode::SourceDialectDisplaced.note(
                cadmpeg_ir::codec::source_dialect_displaced_message(source, target),
            ));
    }
    Ok(ExportPlan::buffered(report, bytes))
}

/// Decide what to write, from the request and the source (design §8.2).
pub(in crate::writer) fn resolve(
    ir: &CadIr,
    request: TargetRequest<'_>,
) -> Result<Resolution, CodecError> {
    let (target, displaced) = match cadmpeg_ir::codec::resolve_write_request(
        ir,
        request,
        dialect::FORMAT,
        dialect::TARGETS,
    )? {
        cadmpeg_ir::codec::WriteRequest::Catalog { entry, displaced } => (
            dialect::written_dialect(dialect::target_options(entry)?),
            displaced,
        ),
        cadmpeg_ir::codec::WriteRequest::OffCatalog { dialect } => (dialect.clone(), None),
    };
    // Deliverability, not preference. This writer patches the retained
    // `Document.xml` and regenerates none, so the resolved target is reachable
    // exactly when the retained graph already declares it — §8.1's "a
    // patch-only writer's row is reachable only from a retained source of that
    // flavor, and the plan refuses by name where it cannot deliver". The
    // refusal is typed and carries the catalog, like every other write refusal;
    // it used to surface as a bare message string from deep inside `write`.
    retained_baseline(ir, &target)
        .map(|options| Resolution { options, displaced })
        .ok_or_else(|| {
            unsupported_target(
                dialect::FORMAT,
                target.as_str(),
                "the retained FCStd document graph does not declare it, and this writer \
                 regenerates no Document.xml, so it cannot be written",
                dialect::TARGETS,
            )
        })
}

/// The write options that reproduce `source_dialect` from the retained document
/// graph, or `None` where that graph cannot carry it.
///
/// The graph is the whole baseline: the writer never regenerates a
/// `Document.xml`, so preservation is possible exactly when the retained
/// document record is present, declares the source's own dialect, and declares
/// it in a form the write options can restate. A `SchemaVersion` of `"04"`
/// classifies as `fcstd:unknown` and does not round-trip through `u32`, so it
/// fails the last condition rather than being rewritten as `"4"`.
fn retained_baseline(ir: &CadIr, source_dialect: &DialectId) -> Option<FcstdWriteOptions> {
    let namespace = ir.native.namespace("fcstd")?;
    let documents = namespace.arena_as::<DocumentFacts>("document").ok()?;
    let [document] = documents.as_slice() else {
        return None;
    };
    let options = FcstdWriteOptions {
        schema_version: document.schema_version.parse().ok()?,
        file_version: document.file_version.parse().ok()?,
    };
    (options.schema_version.to_string() == document.schema_version
        && options.file_version.to_string() == document.file_version
        && dialect::written_dialect(options) == *source_dialect)
        .then_some(options)
}
