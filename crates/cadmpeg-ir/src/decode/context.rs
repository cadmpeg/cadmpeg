// SPDX-License-Identifier: Apache-2.0
//! The decode context: monotonic state, charging, and the session lifecycle.
//!
//! One context exists per decode. It is not `Clone`, and every charging method
//! takes `&self` through interior-mutability cells, so codecs pass
//! `&DecodeContext` and the depth guard composes with recursion. The context
//! owns the budget, the space registry, the ticket table, and the fuse; it
//! never owns bytes (those live in the [`DecodeArena`]) so a `Copy`
//! [`View`] can outlive any single call.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

use crate::codec::{CodecError, DecodeResult, ReadSeek};
use crate::document::Model;
use crate::report::{LossCategory, LossCode, LossNote, ProfileVersions, Severity};

use super::arena::DecodeArena;
use super::budget::BudgetCells;
use super::error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
use super::policy::{DecodeMode, DecodePolicy, Envelope, ResourceLimits};
use super::retained::{
    RetainedAddr, RetainedBlob, RetainedBlobId, RetainedRange, RetainedStore, Retention,
};
use super::space::{ByteRange, SpaceId, SpaceRegistry};
use super::view::{BoundedCount, View};

/// Initial per-expand reservation clamp: an attacker's declared size cannot
/// force a large up-front reservation before any output is produced.
const RESERVE_CLAMP: u64 = 8 * 1024 * 1024;

/// The in-memory charge for one element of type `T`, floored at one byte so a
/// hostile count of zero-sized elements still pays.
fn element_charge<T>() -> u64 {
    std::mem::size_of::<T>().max(1) as u64
}

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    policy: DecodePolicy,
    envelope: Envelope,
    container_only: bool,
    budget: BudgetCells,
    spaces: SpaceRegistry,
    tickets: TicketTable,
    retained: RetainedStore,
    fuse: Cell<Option<ResourceLimit>>,
}

impl<'a> DecodeContext<'a> {
    /// Reads the root input under `max_input_bytes`, copies it into the arena,
    /// registers the root space, establishes input-proportional allowances,
    /// and returns the context and root view.
    pub fn read_root(
        reader: &mut dyn ReadSeek,
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        let max = policy.limits.max_input_bytes;
        let cap = max.saturating_add(1);
        let mut buffer: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let remaining = cap.saturating_sub(buffer.len() as u64);
            if remaining == 0 {
                break;
            }
            let want = usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(chunk.len());
            let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
            if read == 0 {
                break;
            }
            buffer
                .try_reserve(read)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, read as u64))?;
            buffer.extend_from_slice(&chunk[..read]);
        }
        if buffer.len() as u64 > max {
            return Err(root_error(
                ResourceFailure::BudgetExceeded,
                max,
                buffer.len() as u64,
            ));
        }
        let bytes = arena.alloc(buffer.into_boxed_slice());
        Self::from_bytes(bytes, arena, policy)
    }

    /// Builds a context over caller-owned root bytes, for fuzz targets and
    /// tests. The arena still backs any expansions produced during decode.
    pub fn from_root_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes(bytes, arena, policy)
    }

    fn from_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        let length = bytes.len() as u64;
        if length > policy.limits.max_input_bytes {
            return Err(root_error(
                ResourceFailure::BudgetExceeded,
                policy.limits.max_input_bytes,
                length,
            ));
        }
        let budget = BudgetCells::default();
        budget.set_input_basis(length);
        budget.set_counter(ResourceDimension::InputBytes, length);
        let spaces = SpaceRegistry::default();
        let root_space = spaces.register_root();
        let ctx = DecodeContext {
            arena,
            policy: *policy,
            envelope: Envelope::PLATFORM_DEFAULT,
            container_only: false,
            budget,
            spaces,
            tickets: TicketTable::default(),
            retained: RetainedStore::default(),
            fuse: Cell::new(None),
        };
        Ok((ctx, View::over_space(bytes, root_space)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        &self.policy
    }

    /// Returns the failure-handling mode.
    pub fn mode(&self) -> DecodeMode {
        self.policy.mode
    }

    /// Returns whether the caller requested container-only decoding.
    pub fn container_only(&self) -> bool {
        self.container_only
    }

    /// Records the caller's container-only request before decoding begins.
    pub fn set_container_only(&mut self, value: bool) {
        self.container_only = value;
    }

    /// Returns whether the context has fused on a resource failure.
    pub fn is_fused(&self) -> bool {
        self.fuse.get().is_some()
    }

    // --- charging -----------------------------------------------------------

    /// Charges abstract work units, fusing the context on refusal.
    ///
    /// Charge points: commit boundaries (per record), probe scans
    /// (proportionally to the bytes the probe examined, not one unit per
    /// miss), and long-loop charge points.
    pub fn charge_work(
        &self,
        units: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::Work,
            LimitScope::Global,
            units,
            operation,
            location,
        )
    }

    /// Charges bytes retained opaque in salvage mode, fusing on refusal.
    ///
    /// Charge point: salvage-mode opaque retention, before the bytes are
    /// copied into a retained record.
    pub fn charge_retained(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::RetainedBytes,
            LimitScope::Global,
            bytes,
            operation,
            location,
        )
    }

    // --- retained blobs -----------------------------------------------------

    /// Retains an opaque payload as a content-addressed blob, returning a
    /// recoverable [`Retention`].
    ///
    /// Identity is the bytes' SHA-256 digest, so retaining identical bytes twice
    /// deduplicates to one blob and charges the [`RetainedBytes`](ResourceDimension::RetainedBytes)
    /// budget once. The bytes are borrowed for the arena's lifetime, never
    /// re-copied, so the blob survives the context teardown.
    ///
    /// The exhaustion outcome is mode-defined. A fresh blob whose bytes
    /// do not fit the retained budget fails in strict mode with a
    /// `ResourceLimit` (fusing the context) and degrades in salvage mode to
    /// [`Retention::Accounted`]: the digest is kept, the bytes are dropped, a
    /// loss note and the report's `retention_degraded` flag are set at
    /// [`finish`](DecodeContext::finish), and the decode is never failed for
    /// retention alone.
    pub fn retain(&self, bytes: &'a [u8]) -> Result<Retention, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let digest = crate::hash::sha256_hex(bytes);
        let len = bytes.len() as u64;
        let addr = RetainedAddr {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        };
        if self.retained.contains(&digest) {
            return Ok(Retention::Retained(RetainedRange::whole(digest, len)));
        }
        match self.policy.mode {
            DecodeMode::Strict => {
                self.charge_retained(len, "retain", None)?;
                self.retained.insert(digest.clone(), addr);
                Ok(Retention::Retained(RetainedRange::whole(digest, len)))
            }
            DecodeMode::Salvage => {
                if self.try_charge_retained(len) {
                    self.retained.insert(digest.clone(), addr);
                    Ok(Retention::Retained(RetainedRange::whole(digest, len)))
                } else {
                    // Salvage degrades R->A: keep the digest, drop the bytes,
                    // and record the degradation for finish. Keyed by digest so a
                    // later successful retention of the same blob reconciles it.
                    // The context is not fused; retention alone never fails a decode.
                    self.retained.mark_degraded(&digest, len);
                    Ok(Retention::Accounted { digest })
                }
            }
        }
    }

    /// Applies a retained-byte charge without fusing on refusal, returning
    /// whether it fit. The salvage-mode retention path uses this so an exhausted
    /// retained budget degrades to accounting instead of poisoning the decode.
    fn try_charge_retained(&self, bytes: u64) -> bool {
        let dim = ResourceDimension::RetainedBytes;
        let allowance = self
            .budget
            .allowance(dim, &self.policy.limits, &self.envelope);
        let used = self.budget.counter(dim);
        if bytes > allowance.saturating_sub(used) {
            return false;
        }
        self.budget.set_counter(dim, used.saturating_add(bytes));
        true
    }

    /// Returns the retained blobs in canonical order, borrowed for the arena's
    /// lifetime.
    ///
    /// The returned borrows outlive the context: they address the arena the
    /// decode wrapper owns, so a codec collects retained bytes here and they stay
    /// valid after [`finish`](DecodeContext::finish) consumes the context — the
    /// egress copies no bytes.
    pub fn retained_blobs(&self) -> Vec<RetainedBlob<'a>> {
        self.retained
            .addrs()
            .into_iter()
            .map(|(digest, addr)| {
                // SAFETY: `addr` was taken from a `&'a [u8]` passed to `retain`.
                // Those bytes live in the arena (or the caller's root input),
                // which outlives the context and never moves or mutates a stored
                // buffer, so a shared `&'a [u8]` rebuilt from the address is valid
                // for the returned lifetime. This is the same frozen-buffer
                // aliasing the arena relies on.
                let bytes = unsafe { std::slice::from_raw_parts(addr.ptr, addr.len) };
                RetainedBlob {
                    id: RetainedBlobId::new(digest),
                    bytes,
                }
            })
            .collect()
    }

    /// Returns whether any retention degraded from recoverable to accounted
    /// because the retained-byte budget was exhausted in salvage mode.
    pub fn retention_degraded(&self) -> bool {
        self.retained.is_degraded()
    }

    /// Charges the allocation budget for auxiliary heap growth that does not
    /// flow through [`DecodeContext::exact_vec`] or [`DecodeContext::begin_expand`].
    ///
    /// Charge point: per-element runtime graph and summary metadata whose count
    /// tracks an untrusted container directory — a ZIP central directory's entry
    /// count, for one. Each admitted entry pushes a space-graph record, a payload
    /// lookup-map node, and a summary row; that growth is proportional to a count
    /// the input does not bound byte-for-byte, so it is charged here against the
    /// input-proportional allocation allowance rather than left to a codec-local
    /// size ceiling.
    pub fn charge_alloc(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.charge(
            ResourceDimension::AllocBytes,
            LimitScope::Global,
            bytes,
            operation,
            location,
        )
    }

    /// Charges a counter dimension, fusing the context on refusal.
    fn charge(
        &self,
        dim: ResourceDimension,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let allowance = self
            .budget
            .allowance(dim, &self.policy.limits, &self.envelope);
        let used = self.budget.counter(dim);
        if amount > allowance.saturating_sub(used) {
            return Err(self.fuse(
                dim,
                ResourceFailure::BudgetExceeded,
                scope,
                amount,
                operation,
                location,
            ));
        }
        self.budget.set_counter(dim, used.saturating_add(amount));
        Ok(())
    }

    /// Records a permanent fuse and returns the resource error to propagate.
    fn fuse(
        &self,
        dim: ResourceDimension,
        reason: ResourceFailure,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> CodecError {
        let limit = self
            .budget
            .allowance(dim, &self.policy.limits, &self.envelope);
        let used = self.budget.counter(dim);
        let resource = ResourceLimit {
            dimension: dim,
            reason,
            scope,
            limit,
            used,
            additional: amount,
            context: ErrorContext {
                operation,
                location,
            },
        };
        self.fuse.set(Some(resource));
        CodecError::ResourceLimit(resource)
    }

    // --- allocation ---------------------------------------------------------

    /// Reserves capacity for exactly `count` proven elements, charging first.
    ///
    /// The count parameter is [`BoundedCount`], not a raw integer: the caller
    /// must first prove through [`View::counted`] that the elements could
    /// physically fit in unread input. A raw decoded count does not compile:
    ///
    /// ```compile_fail,E0308
    /// # use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy};
    /// # let bytes = [0u8; 8];
    /// # let arena = DecodeArena::new();
    /// # let policy = DecodePolicy::default();
    /// # let (ctx, _root) =
    /// #     DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    /// let declared_count = 0xFFFF_FFFFusize; // untrusted, unproven
    /// let _ = ctx.exact_vec::<u32>(declared_count);
    /// ```
    ///
    /// The builder pushes up to `count` and never reallocates; it exposes no
    /// `DerefMut`, so uncharged growth cannot leak through it.
    pub fn exact_vec<T>(&self, count: BoundedCount) -> Result<ExactVec<T>, CodecError> {
        self.reserve_exact_charged(count.get(), "exact_vec")
    }

    /// Reserves capacity for a zero-floor stream where no physical proof
    /// exists, taking a raw count. The only budget-only allocation path:
    /// charges identically to [`DecodeContext::exact_vec`], and the distinct
    /// greppable name marks the missing input floor at the call site. Sites
    /// keep an explicit codec-local limit as defense in depth.
    pub fn alloc_unfloored<T>(&self, count: usize) -> Result<ExactVec<T>, CodecError> {
        self.reserve_exact_charged(count, "alloc_unfloored")
    }

    fn reserve_exact_charged<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<ExactVec<T>, CodecError> {
        let bytes = (count as u64).saturating_mul(element_charge::<T>());
        self.charge(
            ResourceDimension::AllocBytes,
            LimitScope::Global,
            bytes,
            operation,
            None,
        )?;
        let mut vec = Vec::new();
        vec.try_reserve_exact(count).map_err(|_| {
            self.fuse(
                ResourceDimension::AllocBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                bytes,
                operation,
                None,
            )
        })?;
        Ok(ExactVec {
            vec,
            capacity: count,
        })
    }

    /// Returns an accumulator that charges before each reservation.
    pub fn grow_vec<T>(&self) -> GrowVec<'_, 'a, T> {
        GrowVec {
            ctx: self,
            vec: Vec::new(),
        }
    }

    /// Reads exactly `count.get()` elements through a probe closure, charging
    /// the allocation and refusing a count that could not fit physically.
    ///
    /// A closure miss mid-loop is a committed failure: the read has been
    /// entered, so it is classified, not retried. Classification follows the
    /// deterministic commit rule: fewer than `count.min_element_size()` bytes
    /// remaining at the failure point is `Truncated` (the input ran out);
    /// a miss with at least one more element's bytes still present is an
    /// inconsistency wholly inside the view, `Malformed`.
    pub fn read_counted<'v, T>(
        &self,
        view: &mut View<'v>,
        count: BoundedCount,
        mut read: impl FnMut(&mut View<'v>) -> Option<T>,
    ) -> Result<Vec<T>, CodecError> {
        let mut builder = self.reserve_exact_charged::<T>(count.get(), "read_counted")?;
        for _ in 0..count.get() {
            match read(view) {
                Some(value) => builder.push(value)?,
                None => {
                    let location = view.location();
                    return Err(if view.remaining() < count.min_element_size() {
                        CodecError::Truncated {
                            location,
                            context: ErrorContext {
                                operation: "read_counted",
                                location: Some(location),
                            },
                        }
                    } else {
                        CodecError::Malformed(format!(
                            "read_counted element rejected at space {} offset {} with {} bytes remaining",
                            location.space.index(),
                            location.offset,
                            view.remaining()
                        ))
                    });
                }
            }
        }
        builder.finish_exact()
    }

    // --- decompression ------------------------------------------------------

    /// Begins an expansion whose output is charged incrementally and stored in
    /// the arena; the derived space is registered only on successful finalize.
    pub fn begin_expand(
        &self,
        source: View<'_>,
        spec: ExpandSpec,
    ) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut buffer: Vec<u8> = Vec::new();
        let reserve = match spec {
            ExpandSpec::Exact(size) | ExpandSpec::AtMost(size) => size.min(RESERVE_CLAMP),
            ExpandSpec::Unknown => 0,
        };
        if reserve > 0 {
            let reserve = usize::try_from(reserve).unwrap_or(usize::MAX);
            buffer.try_reserve(reserve).map_err(|_| {
                self.fuse(
                    ResourceDimension::DecompressedBytes,
                    ResourceFailure::AllocationFailed,
                    LimitScope::PerExpand,
                    reserve as u64,
                    "begin_expand",
                    Some(source.location()),
                )
            })?;
        }
        Ok(ExpandWriter {
            ctx: self,
            spec,
            location: source.location(),
            buffer,
            written: 0,
        })
    }

    /// Begins assembling a derived space from several input extents.
    ///
    /// The multi-input sibling of [`DecodeContext::begin_expand`]. Each kind
    /// has distinct charging semantics:
    ///
    /// - [`DerivedKind::Concat`] reassembles the input extents into one logical
    ///   stream (catia extents, iges parameter cards, OLE sectors). The output
    ///   is copied from the inputs here, at construction. Each extent is
    ///   charged to `alloc_bytes` — a fresh heap
    ///   copy of already-accounted bytes, so the input basis does not grow and
    ///   the source extents are not double-counted. The returned writer only
    ///   needs [`DerivedWriter::finalize`].
    /// - [`DerivedKind::Transform`] produces new bytes from the named inputs,
    ///   such as a dictionary-preset decompression that `begin_expand` cannot
    ///   express with its single source view. The caller streams the output
    ///   through [`DerivedWriter::write`], charged to `decompressed_bytes`
    ///   under the per-expand and cumulative decompression ceilings;
    ///   [`DerivedWriter::finalize`] grows the input basis like
    ///   [`ExpandWriter::finalize`], because the output is genuine decompressed
    ///   content the decompression-bomb ceilings must bound.
    ///
    /// The output space registers only on successful
    /// [`DerivedWriter::finalize`] — an incomplete space never escapes, and a
    /// Concat assembly that exceeds `alloc_bytes` fuses here before any writer
    /// escapes.
    pub fn begin_derived_space(
        &self,
        inputs: &[View<'_>],
        kind: DerivedKind,
    ) -> Result<DerivedWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let location = inputs.first().map(|view| view.location());
        let mut writer = DerivedWriter {
            ctx: self,
            kind,
            location,
            buffer: Vec::new(),
            written: 0,
        };
        if matches!(kind, DerivedKind::Concat) {
            // Assemble the output from the input extents now, so the recorded
            // segments are the very bytes copied here and cannot be
            // contradicted later. Each extent charges `alloc_bytes` before it
            // is retained; a refusal fuses the context and no writer escapes.
            for view in inputs {
                writer.append_extent(view.window())?;
            }
        }
        Ok(writer)
    }

    /// Registers a stored (uncompressed) child range as a space that borrows
    /// the parent bytes without copying, returning the new
    /// space id and a view whose coordinates are absolute within it.
    ///
    /// `range` is expressed in the parent view's own space coordinates and must
    /// lie within the parent window; a range that escapes the parent is refused
    /// here, at the request site, exactly as [`View::child`] refuses. No bytes
    /// are copied and no counter is charged: a slice is an alias of input that
    /// was already accounted for when its parent space was admitted, so it
    /// neither grows the input basis nor consumes the allocation budget. It is
    /// the archive-entry counterpart of [`DecodeContext::begin_expand`] —
    /// stored ZIP entries take this path, compressed ones take the expander.
    /// Registration still refuses on a fused context so a stored entry cannot be
    /// admitted after a refusal.
    pub fn register_slice<'v>(
        &self,
        parent: View<'v>,
        range: ByteRange,
    ) -> Result<(SpaceId, View<'v>), CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let start = usize::try_from(range.start).ok();
        let end = usize::try_from(range.end).ok();
        let child = start
            .zip(end)
            .and_then(|(start, end)| parent.child(start, end))
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "stored slice [{}, {}) escapes parent space {}",
                    range.start,
                    range.end,
                    parent.space().index()
                ))
            })?;
        let space = self.spaces.register();
        Ok((space, View::over_space(child.window(), space)))
    }

    // --- depth --------------------------------------------------------------

    /// Enters one recursion level, returning a guard that releases it on drop.
    ///
    /// Depth is a gauge: sequential siblings do not exhaust it. Exceeding the
    /// limit is unswallowable and fuses the context.
    pub fn descend(&self) -> Result<DepthGuard<'_>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let current = self.budget.depth();
        if u64::from(current) >= u64::from(self.policy.limits.max_depth) {
            return Err(self.fuse(
                ResourceDimension::Depth,
                ResourceFailure::BudgetExceeded,
                LimitScope::Global,
                1,
                "descend",
                None,
            ));
        }
        self.budget.set_depth(current.saturating_add(1));
        Ok(DepthGuard {
            budget: &self.budget,
        })
    }

    // --- records ------------------------------------------------------------

    /// Registers a committed record for disposition accounting, returning a
    /// ticket that must be resolved before the decode finishes.
    pub fn commit_record(&self, location: SourceLocation, kind: RecordKind) -> RecordTicket {
        let mut entries = self.tickets.entries.borrow_mut();
        let index = entries.len();
        entries.push(TicketState {
            kind,
            location,
            disposition: None,
        });
        RecordTicket { index }
    }

    /// Resolves a committed record's ticket with its final disposition.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "resolve consumes the ticket so a record cannot be resolved twice"
    )]
    pub fn resolve(&self, ticket: RecordTicket, disposition: RecordDisposition) {
        if let Some(state) = self.tickets.entries.borrow_mut().get_mut(ticket.index) {
            state.disposition = Some(disposition);
        }
    }

    // --- lifecycle ----------------------------------------------------------

    /// Closes the decode, enforcing the session invariants.
    ///
    /// A fused context cannot return `Ok`: the original resource error is
    /// returned even if intermediate code swallowed it through `Option`
    /// chains. On success, every committed ticket must be resolved; an
    /// unresolved ticket is a contract error in strict mode. In salvage mode
    /// each unresolved ticket is resolved as [`RecordDisposition::Dropped`]
    /// and its [`LossNote`] is appended to the result's report, so the
    /// omission stays an accountable outcome.
    ///
    /// Transfer accounting then validates the resolved disposition table: a
    /// [`RecordDisposition::Typed`]
    /// must name at least one output entity, a [`RecordDisposition::Retained`]
    /// must name at least one retained record and every named record must
    /// resolve in the retained store, a [`RecordDisposition::Preserved`] must
    /// name at least one unknown record and every named record must resolve in
    /// the document's native unknowns, and a [`RecordDisposition::Dropped`]'s
    /// loss note must be reflected in the report's losses. A codec that issues
    /// no tickets has an empty table and passes trivially. A violation is a
    /// codec contract error in strict mode (`Malformed`); in salvage mode each
    /// violation is appended as an accountable [`LossNote`] and never fails the
    /// decode, matching the mode's degrade-not-fail discipline.
    pub fn finish(
        self,
        result: Result<DecodeResult, CodecError>,
    ) -> Result<DecodeResult, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let mut result = result?;
        let unresolved = self.tickets.unresolved();
        if unresolved > 0 {
            match self.policy.mode {
                DecodeMode::Strict => {
                    return Err(CodecError::Malformed(format!(
                        "{unresolved} committed record ticket(s) left unresolved"
                    )))
                }
                DecodeMode::Salvage => {
                    let losses = self.tickets.auto_drop_unresolved();
                    result.report.losses.extend(losses);
                }
            }
        }
        // Validate the resolved disposition
        // table against the retained ledger, the document's native unknowns,
        // and the report's losses. Runs after salvage auto-drop so
        // auto-generated Dropped dispositions are checked against the loss
        // notes just appended for them.
        let unknown_ids = match result.ir.all_native_unknowns() {
            Ok(records) => records.into_iter().map(|record| record.id.0).collect(),
            Err(error) => {
                let message = format!("native unknowns arena is undecodable: {error}");
                match self.policy.mode {
                    DecodeMode::Strict => {
                        return Err(CodecError::Malformed(format!(
                            "transfer accounting: {message}"
                        )))
                    }
                    DecodeMode::Salvage => {
                        result.report.losses.push(LossNote {
                            code: LossCode::TransferAccounting,
                            category: LossCategory::Other,
                            severity: Severity::Warning,
                            message: format!("transfer accounting: {message}"),
                            provenance: None,
                        });
                    }
                }
                BTreeSet::new()
            }
        };
        let violations = self.tickets.transfer_accounting(
            &result.ir.model,
            &self.retained,
            &unknown_ids,
            &result.report.losses,
        );
        if !violations.is_empty() {
            match self.policy.mode {
                DecodeMode::Strict => {
                    return Err(CodecError::Malformed(format!(
                        "transfer accounting: {}",
                        violations.join("; ")
                    )))
                }
                DecodeMode::Salvage => {
                    for message in violations {
                        result.report.losses.push(LossNote {
                            code: LossCode::TransferAccounting,
                            category: LossCategory::Other,
                            severity: Severity::Warning,
                            message: format!("transfer accounting: {message}"),
                            provenance: None,
                        });
                    }
                }
            }
        }
        if self.retained.is_degraded() {
            // Salvage degraded recovery to accounting on retained exhaustion:
            // surface it as an accountable outcome — a report flag plus one
            // deterministic loss note, never as a decode failure.
            result.report.retention_degraded = true;
            result.report.losses.push(LossNote {
                code: LossCode::RetentionDegraded,
                category: LossCategory::Other,
                severity: Severity::Warning,
                message: format!(
                    "retained-byte budget exhausted: {} opaque blob(s) totaling {} bytes \
                     degraded from recoverable to accounted",
                    self.retained.degraded_count(),
                    self.retained.degraded_bytes()
                ),
                provenance: None,
            });
        }
        result.report.profile_versions = self.profile_versions();
        Ok(result)
    }

    /// Resolves the limits-profile version, acceptance-envelope version, and
    /// caller ceilings recorded on the decode report.
    ///
    /// The desktop profile is the documented `Default`, so a custom policy's
    /// overrides are its deviations from desktop.
    fn profile_versions(&self) -> ProfileVersions {
        let mut overrides = Vec::new();
        let profile = match self.policy.limits.profile_version() {
            Some(version) => version.to_owned(),
            None => {
                push_limit_overrides(&mut overrides, &self.policy.limits);
                "custom".to_owned()
            }
        };
        overrides.sort();
        ProfileVersions {
            profile,
            envelope: Envelope::VERSION.to_owned(),
            overrides,
        }
    }

    // --- test and instrumentation accessors ---------------------------------

    /// Returns the amount charged against a counter dimension.
    #[cfg(test)]
    pub(crate) fn charged(&self, dim: ResourceDimension) -> u64 {
        self.budget.counter(dim)
    }

    /// Returns the effective allowance for a counter dimension.
    #[cfg(test)]
    pub(crate) fn allowance_of(&self, dim: ResourceDimension) -> u64 {
        self.budget
            .allowance(dim, &self.policy.limits, &self.envelope)
    }

    /// Returns the current input basis.
    #[cfg(test)]
    pub(crate) fn input_basis(&self) -> u64 {
        self.budget.input_basis()
    }

    /// Returns the current recursion depth.
    #[cfg(test)]
    pub(crate) fn current_depth(&self) -> u32 {
        self.budget.depth()
    }

    /// Returns how many spaces are registered.
    #[cfg(test)]
    pub(crate) fn spaces_len(&self) -> usize {
        self.spaces.len()
    }

    /// Returns how many committed tickets remain unresolved.
    #[cfg(test)]
    pub(crate) fn unresolved_tickets(&self) -> usize {
        self.tickets.unresolved()
    }
}

/// Appends a `dimension=value` override descriptor for each ceiling that
/// differs from the desktop default.
///
/// `limits` is destructured by field on purpose: adding a `ResourceLimits`
/// dimension fails to compile here until the new field is listed, so a fresh
/// ceiling cannot silently escape the override record.
fn push_limit_overrides(out: &mut Vec<String>, limits: &ResourceLimits) {
    let base = ResourceLimits::desktop();
    let ResourceLimits {
        max_input_bytes,
        max_decompressed_bytes_total,
        max_decompressed_bytes_per_expand,
        max_alloc_bytes,
        max_work,
        max_depth,
        max_retained_bytes,
    } = *limits;
    let mut push = |name: &str, base_value: u64, value: u64| {
        if value != base_value {
            out.push(format!("{name}={value}"));
        }
    };
    push("max_input_bytes", base.max_input_bytes, max_input_bytes);
    push(
        "max_decompressed_bytes_total",
        base.max_decompressed_bytes_total,
        max_decompressed_bytes_total,
    );
    push(
        "max_decompressed_bytes_per_expand",
        base.max_decompressed_bytes_per_expand,
        max_decompressed_bytes_per_expand,
    );
    push("max_alloc_bytes", base.max_alloc_bytes, max_alloc_bytes);
    push("max_work", base.max_work, max_work);
    push(
        "max_retained_bytes",
        base.max_retained_bytes,
        max_retained_bytes,
    );
    push("max_depth", u64::from(base.max_depth), u64::from(max_depth));
}

/// Builds the root-input resource error before a context exists.
fn root_error(reason: ResourceFailure, limit: u64, used: u64) -> CodecError {
    CodecError::ResourceLimit(ResourceLimit {
        dimension: ResourceDimension::InputBytes,
        reason,
        scope: LimitScope::Global,
        limit,
        used,
        additional: used.saturating_sub(limit),
        context: ErrorContext {
            operation: "read_root",
            location: None,
        },
    })
}

/// A fixed-capacity builder that never reallocates.
///
/// `push` refuses past the reserved capacity so growth stays within the
/// charged budget. `finish_exact` additionally requires full population.
#[derive(Debug)]
pub struct ExactVec<T> {
    vec: Vec<T>,
    capacity: usize,
}

impl<T> ExactVec<T> {
    /// Appends one element, refusing once the reserved capacity is full.
    pub fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.vec.len() < self.capacity {
            self.vec.push(value);
            Ok(())
        } else {
            Err(CodecError::Malformed(format!(
                "exact_vec push past reserved capacity {}",
                self.capacity
            )))
        }
    }

    /// Returns how many elements have been pushed.
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Returns whether no elements have been pushed.
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// Returns the reserved capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Finishes the builder, returning what was pushed.
    pub fn finish(self) -> Vec<T> {
        self.vec
    }

    /// Finishes the builder, requiring the reserved capacity to be filled.
    pub fn finish_exact(self) -> Result<Vec<T>, CodecError> {
        if self.vec.len() == self.capacity {
            Ok(self.vec)
        } else {
            Err(CodecError::Malformed(format!(
                "exact_vec finished with {} of {} elements",
                self.vec.len(),
                self.capacity
            )))
        }
    }
}

/// An accumulator that charges the budget before each reservation.
///
/// `try_push` charges one element, then reserves and pushes. It exposes no
/// `DerefMut`, so uncharged growth cannot leak through it.
#[derive(Debug)]
pub struct GrowVec<'ctx, 'a, T> {
    ctx: &'ctx DecodeContext<'a>,
    vec: Vec<T>,
}

impl<T> GrowVec<'_, '_, T> {
    /// Charges one element, then reserves and pushes it.
    pub fn try_push(&mut self, value: T) -> Result<(), CodecError> {
        let charge = element_charge::<T>();
        self.ctx.charge(
            ResourceDimension::AllocBytes,
            LimitScope::Global,
            charge,
            "grow_vec",
            None,
        )?;
        self.vec.try_reserve(1).map_err(|_| {
            self.ctx.fuse(
                ResourceDimension::AllocBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                charge,
                "grow_vec",
                None,
            )
        })?;
        self.vec.push(value);
        Ok(())
    }

    /// Returns how many elements have been pushed.
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Returns whether no elements have been pushed.
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// Finishes the accumulator, returning what was pushed.
    pub fn finish(self) -> Vec<T> {
        self.vec
    }
}

/// How much output an expansion is expected to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSpec {
    /// A declared exact size, enforced per-write and at finalize.
    Exact(u64),
    /// A declared upper bound only.
    AtMost(u64),
    /// No trustworthy declared size: only the envelope and ceilings apply.
    Unknown,
}

/// How a multi-input derived space assembles its output bytes.
///
/// Selects the charging and construction behavior for a derived space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedKind {
    /// The output is the ordered concatenation of the input extents.
    Concat,
    /// The output is decompressed from the named input extents.
    Transform,
}

/// Writes decompressed output under incremental charging.
#[derive(Debug)]
pub struct ExpandWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    spec: ExpandSpec,
    location: SourceLocation,
    buffer: Vec<u8>,
    written: u64,
}

impl<'a> ExpandWriter<'_, 'a> {
    /// Appends decompressed output, charging before it is retained.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = data.len() as u64;
        let new_written = self.written.saturating_add(len);
        match self.spec {
            ExpandSpec::Exact(size) if new_written > size => {
                return Err(CodecError::Malformed(format!(
                    "expansion exceeded declared exact size {size}"
                )))
            }
            ExpandSpec::AtMost(size) if new_written > size => {
                return Err(CodecError::Malformed(format!(
                    "expansion exceeded declared upper bound {size}"
                )))
            }
            _ => {}
        }
        let per_expand = self
            .ctx
            .budget
            .per_expand_allowance(&self.ctx.policy.limits, &self.ctx.envelope);
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceDimension::DecompressedBytes,
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "expand_write",
                Some(self.location),
            ));
        }
        self.ctx.charge(
            ResourceDimension::DecompressedBytes,
            LimitScope::Global,
            len,
            "expand_write",
            Some(self.location),
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceDimension::DecompressedBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
                len,
                "expand_write",
                Some(self.location),
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = new_written;
        Ok(())
    }

    /// Finalizes the expansion: enforces an exact contract, stores the output
    /// in the arena, registers the derived space, and grows the input basis.
    pub fn finalize(self) -> Result<(SpaceId, View<'a>), CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written != size {
                return Err(CodecError::Malformed(format!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written
                )));
            }
        }
        let length = self.written;
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.spaces.register();
        self.ctx.budget.grow_input_basis(length);
        Ok((space, View::over_space(bytes, space)))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}

/// Assembles a multi-input derived space under incremental charging.
///
/// A [`DerivedKind::Concat`] writer holds the extents copied from its inputs at
/// construction, each charged to `alloc_bytes`; a [`DerivedKind::Transform`]
/// writer takes streamed output through [`DerivedWriter::write`], charged to
/// `decompressed_bytes` under the decompression ceilings. [`DerivedWriter::finalize`]
/// stores the buffer in the arena and registers the space; a Transform
/// additionally grows the input basis.
/// Dropping the writer without finalizing registers nothing.
#[derive(Debug)]
pub struct DerivedWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    kind: DerivedKind,
    location: Option<SourceLocation>,
    buffer: Vec<u8>,
    written: u64,
}

impl<'a> DerivedWriter<'_, 'a> {
    /// Copies one Concat input extent into the buffer, charging `alloc_bytes`
    /// before the bytes are retained. Called during construction so the
    /// bytes are retained.
    fn append_extent(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = data.len() as u64;
        self.ctx.charge(
            ResourceDimension::AllocBytes,
            LimitScope::Global,
            len,
            "derived_concat",
            self.location,
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceDimension::AllocBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                len,
                "derived_concat",
                self.location,
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = self.written.saturating_add(len);
        Ok(())
    }

    /// Appends transform output, charging `decompressed_bytes` under the
    /// per-expand and cumulative decompression ceilings before it is retained.
    ///
    /// Valid only for a [`DerivedKind::Transform`] space, whose output is new
    /// decompressed content that the decompression-bomb ceilings
    /// must bound. A [`DerivedKind::Concat`] space assembles from its declared
    /// inputs at construction and holds no caller-written bytes.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        if !matches!(self.kind, DerivedKind::Transform) {
            return Err(CodecError::Malformed(
                "a derived Concat space assembles from its declared inputs; \
                 write is only for Transform output"
                    .to_owned(),
            ));
        }
        let len = data.len() as u64;
        let new_written = self.written.saturating_add(len);
        let per_expand = self
            .ctx
            .budget
            .per_expand_allowance(&self.ctx.policy.limits, &self.ctx.envelope);
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceDimension::DecompressedBytes,
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "derived_write",
                self.location,
            ));
        }
        self.ctx.charge(
            ResourceDimension::DecompressedBytes,
            LimitScope::Global,
            len,
            "derived_write",
            self.location,
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceDimension::DecompressedBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
                len,
                "derived_write",
                self.location,
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = new_written;
        Ok(())
    }

    /// Finalizes the assembly: stores the output in the arena and registers the
    /// derived space.
    ///
    /// A Transform's output is new decompressed content, so it grows the input
    /// basis like [`ExpandWriter::finalize`]; a Concat reassembles
    /// already-accounted bytes and leaves the basis unchanged.
    pub fn finalize(self) -> Result<(SpaceId, View<'a>), CodecError> {
        let length = self.written;
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let grows_basis = match self.kind {
            DerivedKind::Concat => false,
            DerivedKind::Transform => true,
        };
        let space = self.ctx.spaces.register();
        if grows_basis {
            self.ctx.budget.grow_input_basis(length);
        }
        Ok((space, View::over_space(bytes, space)))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}

/// A recursion-depth guard that releases its level on drop.
#[derive(Debug)]
#[must_use = "binding the guard keeps the depth level held for its scope"]
pub struct DepthGuard<'g> {
    budget: &'g BudgetCells,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.budget.set_depth(self.budget.depth().saturating_sub(1));
    }
}

/// A codec-defined label for a committed record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordKind(pub &'static str);

/// A ticket issued at a commit boundary, resolvable exactly once.
///
/// Construction is confined to [`DecodeContext::commit_record`], and the type
/// is `#[must_use]`; together with the `finish`-time unresolved-ticket check,
/// this makes an accidentally omitted disposition visible in CI.
#[derive(Debug)]
#[must_use = "a committed record ticket must be resolved with ctx.resolve"]
pub struct RecordTicket {
    index: usize,
}

/// The final disposition of a committed record.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordDisposition {
    /// Transferred to typed IR outputs, named by entity id.
    Typed {
        /// The IR entity ids produced.
        outputs: Vec<String>,
    },
    /// Retained opaque, named by retained-record id.
    Retained {
        /// The retained-record ids.
        records: Vec<String>,
    },
    /// Accounted by digest after retained-byte exhaustion.
    Accounted {
        /// Digests recorded without their bytes.
        records: Vec<String>,
    },
    /// Dropped with an accountable loss note.
    Dropped {
        /// Why the record was dropped.
        loss: LossNote,
    },
    /// Preserved verbatim in a native `unknowns` arena, named by
    /// unknown-record id. Distinct from [`RecordDisposition::Retained`], which
    /// names records in the decode-session retained store: a `Preserved`
    /// record's bytes reach the IR through `push_native_unknown`, so its
    /// accounting resolves against the document's native unknowns.
    Preserved {
        /// The native unknown-record ids the record was preserved as.
        records: Vec<String>,
    },
    /// Container framing with no semantic content.
    Structural,
}

#[derive(Debug)]
struct TicketState {
    kind: RecordKind,
    location: SourceLocation,
    disposition: Option<RecordDisposition>,
}

#[derive(Debug, Default)]
struct TicketTable {
    entries: RefCell<Vec<TicketState>>,
}

impl TicketTable {
    fn unresolved(&self) -> usize {
        self.entries
            .borrow()
            .iter()
            .filter(|state| state.disposition.is_none())
            .count()
    }

    /// Validates the resolved disposition table against retained records, model
    /// output, native unknowns, and report losses.
    ///
    /// The checks: a [`RecordDisposition::Typed`] names at least one output
    /// entity and every named entity resolves in `model`; a
    /// [`RecordDisposition::Retained`] names at least one record and
    /// every named record resolves in the retained store; a
    /// [`RecordDisposition::Preserved`] names at least one unknown record and
    /// every named record resolves in `unknown_ids` (the document's native
    /// unknowns); a [`RecordDisposition::Dropped`]'s loss is reflected in
    /// `losses`. An unresolved ticket is not re-reported here —
    /// [`finish`](DecodeContext::finish) handles resolution before this runs.
    /// [`RecordDisposition::Structural`] carries no semantic content and is
    /// unconstrained by retained records and the model.
    ///
    /// Dropped reflection consumes one report loss per disposition: each
    /// `Dropped` claims a distinct un-consumed `LossNote` matching its key, so
    /// N records dropped under a shared key require N notes in `report.losses`.
    /// Existence-only matching would let one note account for many drops,
    /// reintroducing concealed under-accounting.
    fn transfer_accounting(
        &self,
        model: &Model,
        retained: &RetainedStore,
        unknown_ids: &BTreeSet<String>,
        losses: &[LossNote],
    ) -> Vec<String> {
        let mut violations = Vec::new();
        let mut consumed = vec![false; losses.len()];
        for state in self.entries.borrow().iter() {
            let (kind, location) = (state.kind.0, state.location);
            let at = format!(
                "record `{}` at space {} offset {}",
                kind,
                location.space.index(),
                location.offset
            );
            match &state.disposition {
                Some(RecordDisposition::Typed { outputs }) => {
                    if outputs.is_empty() {
                        violations
                            .push(format!("{at} resolved Typed but names no output entities"));
                    }
                    for entity in outputs {
                        if !model.contains_id(entity) {
                            violations.push(format!(
                                "{at} names output entity `{entity}` absent from the IR model"
                            ));
                        }
                    }
                }
                Some(RecordDisposition::Retained { records }) => {
                    if records.is_empty() {
                        violations.push(format!(
                            "{at} resolved Retained but names no retained records"
                        ));
                    }
                    for record in records {
                        if !retained.contains_record(record) {
                            violations.push(format!(
                                "{at} names retained record `{record}` absent from the retained ledger"
                            ));
                        }
                    }
                }
                Some(RecordDisposition::Accounted { records }) => {
                    if records.is_empty() {
                        violations.push(format!("{at} resolved Accounted but names no digests"));
                    }
                    for record in records {
                        if !retained.contains_accounted(record) {
                            violations.push(format!(
                                "{at} names accounted digest `{record}` absent from degraded retention"
                            ));
                        }
                    }
                }
                Some(RecordDisposition::Dropped { loss }) => {
                    let matched = losses.iter().enumerate().find(|(index, note)| {
                        !consumed[*index]
                            && note.code == loss.code
                            && note.category == loss.category
                            && note.message == loss.message
                    });
                    match matched {
                        Some((index, _)) => consumed[index] = true,
                        None => violations.push(format!(
                            "{at} resolved Dropped but its loss note is absent from the report"
                        )),
                    }
                }
                Some(RecordDisposition::Preserved { records }) => {
                    if records.is_empty() {
                        violations.push(format!(
                            "{at} resolved Preserved but names no unknown records"
                        ));
                    }
                    for record in records {
                        if !unknown_ids.contains(record) {
                            violations.push(format!(
                                "{at} names unknown record `{record}` absent from the native unknowns arena"
                            ));
                        }
                    }
                }
                Some(RecordDisposition::Structural) | None => {}
            }
        }
        violations
    }

    /// Resolves every unresolved ticket as `Dropped` with an accountable
    /// loss note, returning the notes for the decode report.
    ///
    /// An unresolved committed record is a record the codec accepted and
    /// never accounted for; it is a loss, never `Structural`.
    fn auto_drop_unresolved(&self) -> Vec<LossNote> {
        let mut notes = Vec::new();
        for state in self.entries.borrow_mut().iter_mut() {
            if state.disposition.is_none() {
                let loss = LossNote {
                    code: LossCode::UnresolvedRecordDropped,
                    category: LossCategory::Other,
                    severity: Severity::Warning,
                    message: format!(
                        "unresolved committed record `{}` at space {} offset {} auto-dropped in salvage mode",
                        state.kind.0,
                        state.location.space.index(),
                        state.location.offset
                    ),
                    provenance: None,
                };
                notes.push(loss.clone());
                state.disposition = Some(RecordDisposition::Dropped { loss });
            }
        }
        notes
    }
}
