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

use crate::codec::{CodecError, DecodeResult, ReadSeek};
use crate::report::{LossCategory, LossNote, ProfileVersions, Severity};

use super::arena::DecodeArena;
use super::budget::BudgetCells;
use super::error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
use super::policy::{DecodeMode, DecodePolicy, Envelope, ResourceLimits};
use super::space::{ByteRange, SourceSpan, SpaceId, SpaceOrigin, SpaceRegistry, TransformKind};
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
        let root_space = spaces.register_root(length);
        let ctx = DecodeContext {
            arena,
            policy: *policy,
            envelope: Envelope::PLATFORM_DEFAULT,
            container_only: false,
            budget,
            spaces,
            tickets: TicketTable::default(),
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
        let input = SourceSpan {
            space: source.space(),
            range: ByteRange {
                start: source.start() as u64,
                end: source.end() as u64,
            },
        };
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
            input,
            location: source.location(),
            buffer,
            written: 0,
        })
    }

    /// Begins assembling a derived space from several input extents.
    ///
    /// The multi-input sibling of [`DecodeContext::begin_expand`]: catia
    /// concatenates extents into a logical stream, iges assembles parameter
    /// cards, OLE storages splice sectors, and a dictionary-preset
    /// decompression draws on two inputs at once. `begin_expand` is the
    /// single-source [`SpaceOrigin::Transform`] special case with
    /// `decompressed_bytes` semantics; this builder covers the multi-input
    /// [`SpaceOrigin::Concat`] and [`SpaceOrigin::Transform`] origins and
    /// charges each write to `alloc_bytes` — the reassembled bytes are a fresh
    /// heap copy, not new decompressed content, so they do not grow the input
    /// basis and are not double-counted against the source extents' spaces.
    ///
    /// The output space registers only on successful [`DerivedWriter::finalize`]
    /// — an incomplete space never escapes.
    pub fn begin_derived_space(
        &self,
        inputs: &[View<'_>],
        kind: DerivedKind,
    ) -> Result<DerivedWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.fuse.get() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let segments = inputs
            .iter()
            .map(|view| SourceSpan {
                space: view.space(),
                range: ByteRange {
                    start: view.start() as u64,
                    end: view.end() as u64,
                },
            })
            .collect();
        let location = inputs.first().map(|view| view.location());
        Ok(DerivedWriter {
            ctx: self,
            kind,
            segments,
            location,
            buffer: Vec::new(),
            written: 0,
        })
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
        result.report.profile_versions = self.profile_versions();
        Ok(result)
    }

    /// Resolves the versioned calibration identifiers recorded on the decode
    /// report (§5.2): the active limits-profile version, the acceptance-
    /// envelope version, and any caller ceilings that deviate from the default.
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

    /// Returns a registered space's derivation origin.
    #[cfg(test)]
    pub(crate) fn space_origin(&self, id: SpaceId) -> Option<SpaceOrigin> {
        self.spaces.origin(id)
    }

    /// Returns how many committed tickets remain unresolved.
    #[cfg(test)]
    pub(crate) fn unresolved_tickets(&self) -> usize {
        self.tickets.unresolved()
    }
}

/// Appends a `dimension=value` override descriptor for each ceiling that
/// differs from the desktop default, the §5.2 baseline for custom policies.
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
/// The [`DecodeContext::begin_derived_space`] counterpart to
/// [`ExpandSpec`]'s size contract: it names the origin the finalized space
/// records, not a size bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedKind {
    /// The output is the ordered concatenation of the input extents, recorded
    /// as [`SpaceOrigin::Concat`].
    Concat,
    /// The output is a transform of the named input extents, recorded as
    /// [`SpaceOrigin::Transform`] with the given kind.
    Transform(TransformKind),
}

/// Writes decompressed output under incremental charging.
#[derive(Debug)]
pub struct ExpandWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    spec: ExpandSpec,
    input: SourceSpan,
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
        let space = self.ctx.spaces.register(
            length,
            SpaceOrigin::Transform {
                inputs: vec![self.input],
                kind: TransformKind::Decompress,
            },
        );
        self.ctx.budget.grow_input_basis(length);
        Ok((space, View::over_space(bytes, space)))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}

/// Assembles a multi-input derived space under incremental `alloc_bytes`
/// charging.
///
/// Each [`DerivedWriter::write`] charges before the bytes are retained;
/// [`DerivedWriter::finalize`] stores the assembled buffer in the arena and
/// registers the space with the [`DerivedKind`]'s origin. Dropping the writer
/// without finalizing registers nothing.
#[derive(Debug)]
pub struct DerivedWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    kind: DerivedKind,
    segments: Vec<SourceSpan>,
    location: Option<SourceLocation>,
    buffer: Vec<u8>,
    written: u64,
}

impl<'a> DerivedWriter<'_, 'a> {
    /// Appends assembled output, charging `alloc_bytes` before it is retained.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = data.len() as u64;
        self.ctx.charge(
            ResourceDimension::AllocBytes,
            LimitScope::Global,
            len,
            "derived_write",
            self.location,
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceDimension::AllocBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                len,
                "derived_write",
                self.location,
            )
        })?;
        self.buffer.extend_from_slice(data);
        self.written = self.written.saturating_add(len);
        Ok(())
    }

    /// Finalizes the assembly: stores the output in the arena and registers the
    /// derived space with its `Concat` or `Transform` origin.
    pub fn finalize(self) -> Result<(SpaceId, View<'a>), CodecError> {
        let length = self.written;
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let origin = match self.kind {
            DerivedKind::Concat => SpaceOrigin::Concat {
                segments: self.segments,
            },
            DerivedKind::Transform(kind) => SpaceOrigin::Transform {
                inputs: self.segments,
                kind,
            },
        };
        let space = self.ctx.spaces.register(length, origin);
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
    /// Dropped with an accountable loss note.
    Dropped {
        /// Why the record was dropped.
        loss: LossNote,
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
