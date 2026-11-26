// SPDX-License-Identifier: Apache-2.0
//! Interfaces for detecting, inspecting, decoding, and encoding CAD formats.
//!
//! [`Codec`] is object-safe for runtime codec registries. Detection consumes a
//! byte prefix, inspection summarizes a seekable container, and decoding
//! produces a finalized [`CadIr`] plus a [`DecodeReport`].
//!
//! A codec implements only the required [`CodecBackend`] methods. The public
//! [`Codec::inspect`] and [`Codec::decode`] entry points are the
//! single enforcement point for root-input limits and session finalize checks;
//! they live on the sealed [`Codec`] trait, blanket-implemented for every
//! [`CodecBackend`], so a codec cannot override an entry point and drop the
//! enforcement.

use std::fmt;

use self::loss::DecodeLossCode;
use crate::document::CadIr;
use crate::report::{
    check::Finding,
    decode::{Coverage, DecodeReport, DecodeTransfer, TransferLedger},
    loss::LossNote as DecodeLoss,
    strictness::Strictness,
};
use crate::source_fidelity::SourceFidelity;
use crate::ContainerSummary;
use cadmpeg_core::decode::{
    ArenaClass, ContentScope, DecodeArena, DecodeContext, DecodeDepth, DecodePolicy,
    InspectOptions, LimitScope, ResourceLimits, View, WholeInput,
};
use cadmpeg_core::dialect::FormatIdentity;
use cadmpeg_core::{CodecError, ReadSeek};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

/// The stable registry format namespace of one codec, as a typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatId(cadmpeg_core::text::StaticCodecId);

impl FormatId {
    /// Builds the format id for a registry namespace word, which is a codec
    /// id (§2.2). In a constant, text of another form fails compilation.
    #[must_use]
    pub const fn new(namespace: &'static str) -> Self {
        match cadmpeg_core::text::StaticCodecId::new(namespace) {
            Some(id) => Self(id),
            None => panic!("a format id is a codec id"),
        }
    }

    /// The registry namespace word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0.as_str()
    }

    /// The registry namespace word as a codec id.
    #[must_use]
    pub const fn codec(self) -> crate::design::names::CodecId {
        crate::design::names::CodecId::from_static(self.0)
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.0.as_str())
    }
}

/// How confident a codec is that it can handle a given byte prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum Confidence {
    /// Definitely not this format.
    No,
    /// Weak signal, such as a generic container signature.
    Low,
    /// Plausible but not conclusive.
    Medium,
    /// Strong, format-specific signal.
    High,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::No => "no",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

/// Options controlling source decoding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecodeOptions {
    /// How much of the source to transfer.
    pub depth: DecodeDepth,
    /// Resource limits governing the decode.
    pub policy: DecodePolicy,
    /// The entries of the completed decode report on which the caller
    /// refuses the decode (§3.3). The default refuses none.
    pub strictness: Strictness,
}

/// A decoded document plus its loss report.
///
/// The [`Codec`] wrapper and archive sessions construct this value after stamping the source
/// classification onto the report and finalizing the IR and source fidelity.
/// `#[non_exhaustive]` blocks external struct literals so callers cannot skip
/// finalization. Read through [`Self::ir`] and [`Self::report`]. Consume with
/// [`Self::into_parts`] to obtain the source fidelity or edit either document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DecodeResult {
    ir: CadIr,
    report: DecodeReport,
    source_fidelity: SourceFidelity,
}

/// A decode refused by the caller's [`Strictness`], bound to the loss in its
/// completed decode report (§3.3).
///
/// Only the shared decode finalizer constructs this value. The refusing loss
/// is therefore always an element of [`Self::report`].
#[derive(Debug)]
pub struct StrictDecodeRejection {
    report: Box<DecodeReport>,
    loss_index: usize,
}

impl StrictDecodeRejection {
    /// Returns the loss that caused the strict-policy refusal.
    #[must_use]
    pub fn loss(&self) -> &DecodeLoss {
        &self.report.losses[self.loss_index]
    }

    /// Returns the completed report that contains the refusing loss.
    #[must_use]
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }
}

impl fmt::Display for StrictDecodeRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loss = self.loss();
        write!(f, "{}: {}", loss.code, loss.message)
    }
}

/// Failure from the policy-enforcing decode entry point.
///
/// Backend and resource failures remain [`CodecError`] values. A strict-policy
/// refusal is separate because decoding completed and produced a report; the
/// caller must be able to serialize that evidence even though no document is
/// admitted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeFailure {
    /// The codec or decode context failed before a result was produced.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// The caller's strictness refused the first loss it names.
    #[error("the caller's strictness refuses the decode at {rejection}")]
    StrictRejected {
        /// The completed report and its refusing loss.
        rejection: StrictDecodeRejection,
    },
}

/// What a backend returns from [`CodecBackend::decode_impl`].
///
/// Source identity is authored once, in `ir.source`. The sealed wrapper stamps
/// that classification onto the report; a backend cannot describe the document
/// and its report with two different identities.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// The decoded document, with its source metadata authored by the backend.
    pub ir: CadIr,
    /// The report without classification.
    pub body: DecodeBody,
    /// Decode-time annotations and retained source records.
    pub source_fidelity: SourceFidelity,
}

/// A [`DecodeReport`] without its classification.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeBody {
    /// The depth the backend delivered and its geometry outcome. The sealed
    /// wrapper records this depth on the document.
    pub transfer: DecodeTransfer,
    /// Coverage measures keyed by their declared name.
    pub coverage: Coverage,
    /// Losses resolved during decoding.
    pub losses: Vec<DecodeLoss>,
    /// Codec-defined informational notes.
    pub notes: Vec<String>,
    /// Complete source-to-result accounting.
    pub transfer_ledger: TransferLedger,
}

impl DecodeBody {
    /// An empty body with the given transfer outcome.
    #[must_use]
    pub fn new(transfer: DecodeTransfer) -> Self {
        Self {
            transfer,
            coverage: Coverage::default(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        }
    }

    /// Charges `decode/depth.not-delivered` for `class` where the source
    /// holds records of `class` that the delivered depth leaves out, as the
    /// joints of a `Geometry`-depth decode (§2.1). A class the delivered
    /// depth holds is not charged, and a class is charged once.
    pub fn source_holds(&mut self, class: ArenaClass) {
        let depth = self.transfer.depth();
        if depth.scope().holds(class) {
            return;
        }
        let note = held_beyond_depth(depth, class);
        if !self.losses.contains(&note) {
            self.losses.push(note);
        }
    }
}

/// The loss for records of `class` that the source holds and a decode at
/// `depth` leaves out.
fn held_beyond_depth(depth: DecodeDepth, class: ArenaClass) -> DecodeLoss {
    DecodeLossCode::DepthNotDelivered { class }.note(format!(
        "the source holds {class} records that a {depth}-depth decode does not deliver, so they \
         and the references to them are absent"
    ))
}

impl DecodeResult {
    /// Finalizes backend output with classification and depth, then applies
    /// the caller's `strictness` to the completed report. A decode at
    /// `Container` depth exempts [`Strictness::pinned`].
    /// The caller must close the context with `finish_session`, including on error.
    pub fn from_decoded(
        ctx: &DecodeContext<'_>,
        decoded: Decoded,
        format: FormatId,
        strictness: &Strictness,
    ) -> Result<Self, DecodeFailure> {
        Self::from_decoded_inner(ctx, decoded, format, strictness, true)
    }

    /// Finalizes backend output with classification, depth, normalization, and
    /// the caller's strict-loss policy without computing canonical JSON
    /// content or omitted-arena digests. The digest walk finds the records
    /// past the CADIR bounds, so this path gives up none of them.
    ///
    /// Intended for internal read-back validation that inspects only `ir()`
    /// and discards `source_fidelity().content_digest`.
    pub fn from_decoded_without_digests(
        ctx: &DecodeContext<'_>,
        decoded: Decoded,
        format: FormatId,
        strictness: &Strictness,
    ) -> Result<Self, DecodeFailure> {
        Self::from_decoded_inner(ctx, decoded, format, strictness, false)
    }

    fn from_decoded_inner(
        ctx: &DecodeContext<'_>,
        decoded: Decoded,
        format: FormatId,
        strictness: &Strictness,
        record_digests: bool,
    ) -> Result<Self, DecodeFailure> {
        let mut result = DecodeResult::new(decoded, format, ctx.depth(), ctx.policy())?;
        result.name_dropped_records(ctx)?;
        result.give_up_repeated_ids(ctx, format)?;
        if !ctx.container_only() {
            result.widen_vertex_tolerances(ctx.policy());
        }
        if record_digests {
            result.record_content_digest(ctx, format)?;
            result.record_omitted_arenas_baseline(ctx.policy());
        }
        if result.report().format() != format.as_str() {
            return Err(CodecError::WrongFormat(format!(
                "codec {:?} decoded a {:?} document",
                format.as_str(),
                result.report().format()
            ))
            .into());
        }
        if let Some(loss_index) =
            strictness.first_refused(&result.report().losses, ctx.container_only())
        {
            let (_, report, _) = result.into_parts();
            let rejection = StrictDecodeRejection {
                report: Box::new(report),
                loss_index,
            };
            return Err(DecodeFailure::StrictRejected { rejection });
        }
        Ok(result)
    }

    /// Builds a result by stamping the document's source classification onto
    /// the report body, then canonicalizing the IR and source fidelity: every
    /// arena sorted by id, then every one-spelling rule in its canonical form
    /// (§3.1).
    ///
    /// A document without source metadata yields an unclassified report for
    /// `format`, the codec's registry format. The document records the scope
    /// of the depth the backend's transfer state reports, less each class
    /// `requested` excludes that the document leaves empty; each class
    /// `requested` names that the document lacks adds an error loss, unless
    /// the body already charges it through [`DecodeBody::source_holds`].
    ///
    /// The normalizer runs under `policy`, the decode's own. Only a refusal
    /// that stops the session refuses the decode; where the normalizer gives
    /// records up, or stops for any other reason, the document is kept as it
    /// stands and a `decode/normalize.incomplete` Warning names what it left
    /// (interim, inventory gap 20261119-N1-4).
    fn new(
        decoded: Decoded,
        format: FormatId,
        requested: DecodeDepth,
        policy: &DecodePolicy,
    ) -> Result<Self, CodecError> {
        let Decoded {
            mut ir,
            mut body,
            source_fidelity,
        } = decoded;
        let scope = recorded_scope(&ir, body.transfer.depth().scope(), requested.scope());
        ir.set_scope(scope);
        for class in scope.missing_from(requested.scope()) {
            if body
                .losses
                .contains(&held_beyond_depth(body.transfer.depth(), class))
            {
                continue;
            }
            body.losses
                .push(DecodeLossCode::DepthNotDelivered { class }.note(format!(
                    "requested {requested} depth but the {format} decoder did not deliver \
                     the {class} class, so its records are absent"
                )));
        }
        let classification = match ir.source.as_ref() {
            Some(source) => match source.classification() {
                FormatIdentity::Classified { dialects } => {
                    FormatIdentity::classified(dialects.clone())
                }
                FormatIdentity::Unclassified { format } => {
                    FormatIdentity::unclassified(format.clone())
                }
            },
            None => FormatIdentity::unclassified(format.codec()),
        };
        ir.finalize();
        match crate::design::normalize::normalize(&mut ir, policy) {
            Ok(given_up) if given_up.is_empty() => {}
            Ok(given_up) => body
                .losses
                .push(DecodeLossCode::NormalizeIncomplete.note(format!(
                    "the decode wrapper's normalizer left part of {} in the spelling the decoder \
                 stated: its selections or constructions nest past the recursion ceiling",
                    given_up.join(", ")
                ))),
            Err(CodecError::ResourceLimit(limit)) if limit.scope == LimitScope::Session => {
                return Err(CodecError::ResourceLimit(limit));
            }
            Err(error) => body
                .losses
                .push(DecodeLossCode::NormalizeIncomplete.note(format!(
                    "the decode wrapper's normalizer stopped ({error}), so the document keeps \
                 spellings the decoder stated"
                ))),
        }
        Ok(Self {
            ir,
            report: DecodeReport::from_body(classification, body),
            source_fidelity,
        })
    }

    /// Records the digest of the arenas the CADIR wire omits as the
    /// document returns, beside the content digest, which covers the wire
    /// form alone. Where it cannot be taken, the content digest decides alone.
    fn record_omitted_arenas_baseline(&mut self, policy: &DecodePolicy) {
        let arena = DecodeArena::new();
        let Ok((digest_ctx, _)) = DecodeContext::from_root_bytes(&[], &arena, policy) else {
            return;
        };
        if let Ok(digest) = crate::hash::omitted_arenas_local_sha256(&digest_ctx, &self.ir) {
            self.ir.model.record_omitted_arenas_baseline(digest);
        }
    }

    /// States one copy of each id a decoder emitted more than once in one
    /// arena where the copies hold one value, and none where they differ
    /// (§2.8): the id is given up as a part (§3.3), and one
    /// `decode/id.repeated` loss names it, the arena, the number of copies,
    /// its stub, and the records its give-up drops and marks.
    fn give_up_repeated_ids(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: FormatId,
    ) -> Result<(), CodecError> {
        let repeated = self.ir.repeated_ids(crate::document::DifferingCopies::Keep);
        if repeated.is_empty() {
            return Ok(());
        }
        let copies: std::collections::BTreeMap<crate::give_up::Seed, usize> = repeated
            .into_iter()
            .map(|repeated| {
                (
                    crate::give_up::Seed {
                        arena: repeated.arena,
                        id: repeated.id,
                    },
                    repeated.copies,
                )
            })
            .collect();
        let losses = crate::give_up::give_up(
            ctx,
            &mut self.ir,
            &mut self.source_fidelity,
            format.as_str(),
            copies.keys().cloned().collect(),
            |seed, released, retained| {
                DecodeLossCode::IdRepeated.note(format!(
                    "the decoder emitted {} {} times in arena {}, and the copies differ, so the \
                     document states none of them; record={}; {}",
                    seed.id,
                    copies.get(seed).copied().unwrap_or_default(),
                    seed.arena,
                    retained.stub,
                    released.tail(&[&seed.id])
                ))
            },
        )?;
        self.report.losses.extend(losses);
        Ok(())
    }

    /// States the tolerance of each vertex the source stores farther from a
    /// position part 10 compares it with than the coincidence allowance
    /// there, and charges the pass's losses (§2.3).
    fn widen_vertex_tolerances(&mut self, policy: &DecodePolicy) {
        let losses =
            vertex_tolerance::widen(&mut self.ir, &mut self.source_fidelity.annotations, policy);
        self.report.losses.extend(losses);
    }

    /// Records the SHA-256 of the canonical JSON of the document the decode
    /// returns as the sidecar's `content_digest` (§3.1, Rule B). The digest
    /// walks the document in a session of its own under the decode's
    /// policy, with a recursion ceiling of at least
    /// [`crate::bounds::DIGEST_WALK_DEPTH`], so a ceiling it meets refuses no
    /// part of the decode. The records it gives up retain their text in
    /// `ctx`, the decode's session.
    ///
    /// The same walk finds each record the decoder stated past a bound of
    /// §2.5, which §2.8 states none of. Each expression past the bound of an
    /// expression is stated `Source` with no tokens, the form §3.3 gives an
    /// expression of which nothing is read, with a `record.undecoded` Warning
    /// that names its record. Each other such record is given up as a part
    /// (§3.3), with a `record.undecoded` Warning that names it and the
    /// records its give-up drops and marks, and the document is digested
    /// again without it. Each value outside every record past a bound is
    /// stated as the value it takes when unstated, with a `record.undecoded`
    /// Warning that names its field. The walk admits
    /// at least [`crate::bounds::DIGEST_WALK_DEPTH`] levels, whatever the policy's
    /// recursion ceiling, so that it digests a document within the bound and
    /// writes the first container past it before it is refused. A refused
    /// walk stops in the record it was refused in, so the wrapper walks again
    /// after each give-up.
    fn record_content_digest(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: FormatId,
    ) -> Result<(), CodecError> {
        let policy = DecodePolicy {
            limits: ResourceLimits {
                max_recursion_depth: ctx
                    .policy()
                    .limits
                    .max_recursion_depth
                    .max(crate::bounds::DIGEST_WALK_DEPTH),
                ..ctx.policy().limits
            },
        };
        let walk = |ir: &CadIr| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            Ok::<_, CodecError>(crate::hash::measured_content_digest(&ctx, ir))
        };
        let (mut digest, mut measured) = walk(&self.ir)?;
        if self.restate_expressions_past_the_bound(&measured.records) {
            (digest, measured) = walk(&self.ir)?;
        }
        while self.give_up_records_past_the_bounds(ctx, format, &measured.records)? {
            (digest, measured) = walk(&self.ir)?;
        }
        if !measured.outside.is_empty() {
            for unstated in crate::bounds::unstate_outside(&mut self.ir, &measured.outside)? {
                self.report
                    .losses
                    .push(crate::unknown::part::Cause::Undecoded.note(format!(
                        "the decode wrapper states {} ({})",
                        unstated.what, unstated.field
                    )));
            }
            (digest, measured) = walk(&self.ir)?;
            while self.give_up_records_past_the_bounds(ctx, format, &measured.records)? {
                (digest, measured) = walk(&self.ir)?;
            }
        }
        if let Some(bound) = measured.outside.values().next() {
            let fields: std::collections::BTreeSet<&str> = measured
                .outside
                .keys()
                .map(|outside| outside.field.as_str())
                .collect();
            self.report
                .losses
                .push(crate::unknown::part::Cause::Undecoded.note(format!(
                    "the document {} outside every record ({}), which the decode wrapper keeps",
                    bound.describe(),
                    fields.into_iter().collect::<Vec<_>>().join(", ")
                )));
        }
        self.source_fidelity.content_digest = Some(digest.map_err(CodecError::from)?);
        Ok(())
    }

    /// States `Source` with no tokens each expression past its bound in the
    /// records of `records` that pass that bound, the partial form §3.3 gives
    /// an expression, so that each such record and each record that needs it
    /// stays, and returns whether it restated one. Each restated record
    /// charges one `record.undecoded` Warning that names it.
    fn restate_expressions_past_the_bound(
        &mut self,
        records: &std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
    ) -> bool {
        if !records
            .values()
            .any(|bound| *bound == crate::bounds::Bound::Expression)
        {
            return false;
        }
        self.ir.finalize();
        let seeds: Vec<crate::give_up::Seed> = records
            .iter()
            .filter(|(_, bound)| **bound == crate::bounds::Bound::Expression)
            .filter_map(|(place, _)| self.ir.record_at(place))
            .collect();
        let restated = crate::give_up::restate_past_the_bound(&mut self.ir, &seeds);
        for seed in &restated {
            self.report
                .losses
                .push(crate::unknown::part::Cause::Undecoded.note(format!(
                    "record {} of arena {} {}, so the decode wrapper states each such \
                     expression Source with no tokens",
                    seed.id,
                    seed.arena,
                    crate::bounds::Bound::Expression.describe()
                )));
        }
        !restated.is_empty()
    }

    /// Gives up each record of `records` as a part (§3.3) and returns
    /// whether it gave up any. The places index the arenas in identity
    /// order, so the arenas are put in that order first.
    fn give_up_records_past_the_bounds(
        &mut self,
        ctx: &DecodeContext<'_>,
        format: FormatId,
        records: &std::collections::BTreeMap<crate::bounds::Place, crate::bounds::Bound>,
    ) -> Result<bool, CodecError> {
        if records.is_empty() {
            return Ok(false);
        }
        self.ir.finalize();
        let bounds: std::collections::BTreeMap<crate::give_up::Seed, String> = records
            .iter()
            .filter_map(|(place, bound)| {
                self.ir
                    .record_at(place)
                    .map(|seed| (seed, bound.describe()))
            })
            .collect();
        let losses = crate::give_up::give_up(
            ctx,
            &mut self.ir,
            &mut self.source_fidelity,
            format.as_str(),
            bounds.keys().cloned().collect(),
            |seed, released, retained| {
                crate::unknown::part::Cause::Undecoded.note(format!(
                    "part {} path={:?} length={} record={}: record {} of arena {} {}, so the \
                     decode wrapper gave it up; {}",
                    seed.arena.kind(),
                    released.path,
                    retained.length,
                    retained.stub,
                    seed.id,
                    seed.arena,
                    bounds.get(seed).map_or("", String::as_str),
                    released.tail(&[&seed.id])
                ))
            },
        )?;
        let gave_up = !losses.is_empty();
        self.report.losses.extend(losses);
        Ok(gave_up)
    }

    /// Borrow the finalized IR.
    pub fn ir(&self) -> &CadIr {
        &self.ir
    }

    /// Borrow the transfer report.
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }

    /// Borrow the decode-time source fidelity.
    pub fn source_fidelity(&self) -> &SourceFidelity {
        &self.source_fidelity
    }

    /// Consume into IR, report, and source fidelity.
    pub fn into_parts(self) -> (CadIr, DecodeReport, SourceFidelity) {
        (self.ir, self.report, self.source_fidelity)
    }
}

/// Decoder and container inspector for one source format.
pub trait CodecBackend {
    /// Registry format namespace this codec decodes, e.g. `"f3d"`.
    ///
    /// The sealed wrapper reports it as [`Codec::id`] and refuses any result
    /// whose primary format names another namespace.
    const FORMAT: FormatId;

    /// Findings this codec's own validator reports over its native
    /// namespace on a decoded document.
    ///
    /// The default reports nothing, for a codec that writes no native
    /// namespace or has no validator for it. The sealed wrapper exposes it as
    /// [`Codec::validate_native`], which the application runs after
    /// `validate_neutral`.
    ///
    /// `source` borrows the document's decode fidelity when available. `None`
    /// is supported for CADIR loaded without a sidecar. A present record can
    /// retain only a digest; validators must inspect its `data()` before
    /// comparing native metadata with source bytes and report skipped scopes.
    fn validate_native(
        _ctx: &DecodeContext<'_>,
        _ir: &CadIr,
        _source: Option<&SourceFidelity>,
    ) -> Result<Vec<Finding>, CodecError> {
        Ok(Vec::new())
    }

    /// Replace source text while retaining codec-owned lexical constraints.
    fn repro_redactions(
        _ir: &CadIr,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError> {
        Ok(std::collections::BTreeMap::new())
    }

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        prefix: View<'_>,
    ) -> Result<Confidence, CodecError>;

    /// Judge the whole source, after the registry proved that the detection
    /// prefix holds every byte of it. The default judges it as a prefix.
    fn detect_whole_impl(
        &self,
        ctx: &DecodeContext<'_>,
        input: WholeInput<'_>,
    ) -> Result<Confidence, CodecError> {
        self.detect_impl(ctx, input.view())
    }

    /// Enumerate the acquired root view's streams/segments without decoding
    /// geometry.
    ///
    /// Implemented by each codec; never called by the CLI or registry. The
    /// [`Codec::inspect`] wrapper acquires the root under the inspection's
    /// input limit and runs this under an internal context.
    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decode the acquired root view, reporting incomplete or approximate
    /// transfer.
    ///
    /// Implemented by each codec; never called by the CLI or registry. The
    /// [`Codec::decode`] wrapper acquires the root and finalizes the
    /// context around this call.
    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError>;
}

mod sealed {
    /// Private bound for the blanket [`Codec`](super::Codec) implementation.
    pub trait Sealed {}
    impl<C: super::CodecBackend + ?Sized> Sealed for C {}
}

/// Public inspection and decoding entry points.
///
/// ```compile_fail
/// use cadmpeg_ir::codec::{
///     Codec, CodecBackend, Confidence, DecodeOptions, DecodeResult, Decoded, FormatId,
/// };
/// use cadmpeg_core::{CodecError, ReadSeek};
/// use cadmpeg_core::decode::{DecodeContext, View};
/// use cadmpeg_core::decode::InspectOptions;
/// use cadmpeg_ir::ContainerSummary;
///
/// struct Rogue;
/// impl CodecBackend for Rogue {
///     const FORMAT: FormatId = FormatId::new("rogue");
///     fn detect_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Confidence, CodecError> { Ok(Confidence::No) }
///     fn inspect_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>)
///         -> Result<Decoded, CodecError> { panic!("never runs") }
/// }
/// impl Codec for Rogue {
///     fn id(&self) -> FormatId { FormatId::new("rogue") }
///     fn detect(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Confidence, CodecError> { Ok(Confidence::No) }
///     fn validate_native(&self, _: &DecodeContext<'_>, _: &cadmpeg_ir::CadIr, _: Option<&cadmpeg_ir::SourceFidelity>)
///         -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
///         Ok(Vec::new())
///     }
///     fn inspect(&self, _: &mut dyn ReadSeek, _: &InspectOptions)
///         -> Result<ContainerSummary, CodecError> { panic!("never runs") }
///     fn decode(&self, _: &mut dyn ReadSeek, _: &DecodeOptions)
///         -> Result<DecodeResult, cadmpeg_ir::codec::DecodeFailure> {
///         panic!("never runs")
///     }
/// }
/// ```
pub trait Codec: sealed::Sealed {
    /// Registry format namespace, [`CodecBackend::FORMAT`].
    fn id(&self) -> FormatId;

    /// Judge, from a leading byte prefix, whether this codec applies.
    fn detect(&self, ctx: &DecodeContext<'_>, prefix: View<'_>) -> Result<Confidence, CodecError>;

    /// Judge the whole source, [`CodecBackend::detect_whole_impl`].
    fn detect_whole(
        &self,
        ctx: &DecodeContext<'_>,
        input: WholeInput<'_>,
    ) -> Result<Confidence, CodecError>;

    /// Findings this codec reports over its own native namespace,
    /// [`CodecBackend::validate_native`].
    fn validate_native(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        source: Option<&SourceFidelity>,
    ) -> Result<Vec<Finding>, CodecError>;

    /// Codec-owned lexical replacements for private reproduction fragments.
    fn repro_redactions(
        &self,
        ir: &CadIr,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError>;

    /// Inspects the source under its input and resource limits.
    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError>;

    /// Decodes the source under its input and resource limits.
    ///
    /// `options.strictness` refuses the completed decode with
    /// [`DecodeFailure::StrictRejected`] at the first loss it refuses. A
    /// decode at `Container` depth exempts [`Strictness::pinned`]. This gate
    /// owns the refusal predicate and the refusal class for every codec. A
    /// backend reports its losses with their strict floors and adds no strict
    /// gate of its own.
    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, DecodeFailure>;

    /// Decodes an acquired root in the caller's detection session, under the
    /// session's policy and requested depth, and applies `strictness` as
    /// [`Self::decode`] does.
    /// The caller finishes the session after this method returns.
    fn decode_with_context(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
        strictness: &Strictness,
    ) -> Result<DecodeResult, DecodeFailure>;
}

impl<C: CodecBackend + ?Sized> Codec for C {
    fn id(&self) -> FormatId {
        C::FORMAT
    }

    fn detect(&self, ctx: &DecodeContext<'_>, prefix: View<'_>) -> Result<Confidence, CodecError> {
        self.detect_impl(ctx, prefix)
    }

    fn detect_whole(
        &self,
        ctx: &DecodeContext<'_>,
        input: WholeInput<'_>,
    ) -> Result<Confidence, CodecError> {
        self.detect_whole_impl(ctx, input)
    }

    fn validate_native(
        &self,
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        source: Option<&SourceFidelity>,
    ) -> Result<Vec<Finding>, CodecError> {
        C::validate_native(ctx, ir, source)
    }

    fn repro_redactions(
        &self,
        ir: &CadIr,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError> {
        C::repro_redactions(ir)
    }

    fn inspect(
        &self,
        reader: &mut dyn ReadSeek,
        options: &InspectOptions,
    ) -> Result<ContainerSummary, CodecError> {
        let arena = DecodeArena::new();
        let policy = DecodePolicy {
            limits: options.limits,
        };
        let (ctx, root) = DecodeContext::read_root(reader, &arena, &policy, DecodeDepth::Full)?;
        let result = self.inspect_impl(&ctx, root);
        ctx.finish_session()?;
        let result = result?;
        if result.format() != C::FORMAT.as_str() {
            return Err(CodecError::WrongFormat(format!(
                "codec {:?} inspected a {:?} container",
                C::FORMAT.as_str(),
                result.format()
            )));
        }
        Ok(result)
    }

    fn decode(
        &self,
        reader: &mut dyn ReadSeek,
        options: &DecodeOptions,
    ) -> Result<DecodeResult, DecodeFailure> {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::read_root(reader, &arena, &options.policy, options.depth)?;
        let result = self.decode_with_context(&ctx, root, &options.strictness);
        ctx.finish_session()?;
        result
    }

    fn decode_with_context(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
        strictness: &Strictness,
    ) -> Result<DecodeResult, DecodeFailure> {
        let decoded = self.decode_impl(ctx, root)?;
        DecodeResult::from_decoded(ctx, decoded, C::FORMAT, strictness)
    }
}

mod dropped;
pub(crate) mod loss;
mod vertex_tolerance;
pub mod write;

#[cfg(test)]
mod tests;

/// The scope a decode records: the delivered scope, less each class the
/// request excludes whose arenas the document leaves empty.
fn recorded_scope(ir: &CadIr, delivered: ContentScope, requested: ContentScope) -> ContentScope {
    ArenaClass::ALL
        .into_iter()
        .rev()
        .fold(delivered, |scope, class| {
            let withdrawn = scope.with(class, false);
            if scope.holds(class)
                && !requested.holds(class)
                && ir.populated_arena(class).is_none()
                && withdrawn.unmet_requirement().is_none()
            {
                withdrawn
            } else {
                scope
            }
        })
}
