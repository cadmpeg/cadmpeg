// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::{Cell, RefCell};
use std::io::SeekFrom;

use crate::{CodecError, ReadSeek};

use super::arena::DecodeArena;
use super::budget::{alloc_filled, DecodeBudget, DepthGuard, ScopedReservation, WorkBudget};
use super::error::{
    ErrorContext, LimitScope, ResourceDimension, ResourceFailure, ResourceLimit, SourceLocation,
};
use super::policy::{
    DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
};
use super::space::{
    resolve_address, ByteRange, ResolvedAddress, SpaceDerivation, SpaceDescriptor, SpaceId,
};
use super::view::View;

/// Cap on the initial per-expand reservation before any output is produced.
const RESERVE_CLAMP: u64 = 8 * 1024 * 1024;

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    policy: DecodePolicy,
    container_only: bool,
    budget: DecodeBudget,
    next_space: Cell<u32>,
    spaces: RefCell<Vec<SpaceDescriptor>>,
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
        let buffer = if let Ok(size) = reader
            .seek(SeekFrom::End(0))
            .and_then(|size| reader.rewind().map(|()| size))
        {
            let reserve = size.min(cap);
            let reserve = usize::try_from(reserve)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, reserve))?;
            let mut buffer = Vec::new();
            buffer
                .try_reserve(reserve)
                .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, reserve as u64))?;
            let mut chunk = vec![0u8; 256 * 1024].into_boxed_slice();
            while (buffer.len() as u64) < cap {
                let remaining = cap.saturating_sub(buffer.len() as u64);
                let want =
                    usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(chunk.len());
                let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            buffer
        } else {
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let remaining = cap.saturating_sub(buffer.len() as u64);
                if remaining == 0 {
                    break;
                }
                let want =
                    usize::try_from(remaining.min(chunk.len() as u64)).unwrap_or(chunk.len());
                let read = reader.read(&mut chunk[..want]).map_err(CodecError::Io)?;
                if read == 0 {
                    break;
                }
                buffer
                    .try_reserve(read)
                    .map_err(|_| root_error(ResourceFailure::AllocationFailed, max, read as u64))?;
                buffer.extend_from_slice(&chunk[..read]);
            }
            buffer
        };
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
        let ctx = DecodeContext {
            arena,
            policy: *policy,
            container_only: false,
            budget: DecodeBudget::new(*policy, length),
            next_space: Cell::new(1),
            spaces: RefCell::new(vec![SpaceDescriptor {
                id: SpaceId::ROOT,
                label: "root".into(),
                derivation: SpaceDerivation::Root,
            }]),
        };
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        &self.policy
    }

    /// Returns whether the caller requested container-only decoding.
    pub fn container_only(&self) -> bool {
        self.container_only
    }

    /// Records the caller's container-only request before decoding begins.
    pub fn set_container_only(&mut self, value: bool) {
        self.container_only = value;
    }

    fn decompression_allowance(&self) -> u64 {
        self.budget.decompression_allowance()
    }

    fn per_expand_allowance(&self) -> u64 {
        let proportional = DECOMPRESSED_PER_EXPAND_BASE.saturating_add(
            DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE.saturating_mul(self.budget.input_bytes()),
        );
        self.policy
            .limits
            .max_decompressed_bytes_per_expand
            .min(proportional)
    }

    fn allocate_space(&self, label: String, derivation: SpaceDerivation) -> SpaceId {
        let index = self.next_space.get();
        self.next_space.set(index.saturating_add(1));
        let id = SpaceId::from_index(index);
        self.spaces.borrow_mut().push(SpaceDescriptor {
            id,
            label,
            derivation,
        });
        id
    }

    /// Returns the stable descriptors registered for this decode session.
    pub fn space_descriptors(&self) -> Vec<SpaceDescriptor> {
        self.spaces.borrow().clone()
    }

    /// Resolves a session-local location into an owned root-to-leaf address.
    pub fn resolve_location(&self, location: SourceLocation) -> ResolvedAddress {
        resolve_address(&self.spaces.borrow(), location)
    }

    fn charge_decompressed(
        &self,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        debug_assert_eq!(scope, LimitScope::Global);
        self.budget.charge_decompressed(amount, operation, location)
    }

    /// Records a permanent fuse and returns the resource error to propagate.
    fn fuse(
        &self,
        reason: ResourceFailure,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> CodecError {
        let limit = match scope {
            LimitScope::Global => self.decompression_allowance(),
            LimitScope::PerExpand => self.per_expand_allowance(),
        };
        self.budget.refuse(
            ResourceDimension::DecompressedBytes,
            reason,
            scope,
            limit,
            self.budget.decompressed_used(),
            amount,
            operation,
            location,
        )
    }

    /// Reserves bytes held by a temporary materialization.
    pub fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.budget.reserve_scoped(bytes, operation, location)
    }

    /// Charges bytes retained for the remainder of this session.
    pub fn charge_retained(
        &self,
        bytes: u64,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<(), CodecError> {
        self.budget.charge_retained(bytes, operation, location)
    }

    /// Copies bytes into session-retained storage after charging and reserving safely.
    pub fn copy_retained(
        &self,
        bytes: &[u8],
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<Vec<u8>, CodecError> {
        self.charge_retained(bytes.len() as u64, operation, location)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(bytes.len()).map_err(|_| {
            self.fuse(
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                bytes.len() as u64,
                operation,
                location,
            )
        })?;
        copy.extend_from_slice(bytes);
        Ok(copy)
    }

    /// Allocates `count` copies of `value` after charging collection items and
    /// reserving without panicking on allocator refusal.
    ///
    /// Prefer this over `vec![value; parsed_count]` for attacker-influenced sizes.
    pub fn alloc_filled<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_collection_items(count as u64, operation)?;
        alloc_filled(count, value, operation)
    }

    /// Charges admitted entities.
    pub fn charge_entities(&self, count: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_entities(count, operation)
    }

    /// Charges entities newly present since `admitted`, then advances `admitted`.
    ///
    /// Codecs call this at admission boundaries so `max_entities` refuses further
    /// work instead of only reporting after a finished IR is built.
    pub fn admit_entities(
        &self,
        current: u64,
        admitted: &mut u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let additional = current.saturating_sub(*admitted);
        self.charge_entities(additional, operation)?;
        *admitted = current;
        Ok(())
    }

    /// Charges admitted collection items.
    pub fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.budget.charge_collection_items(count, operation)
    }

    /// Enters one recursive nesting level until the returned guard is dropped.
    pub fn enter_nested(
        &self,
        operation: &'static str,
        location: Option<SourceLocation>,
    ) -> Result<DepthGuard<'_>, CodecError> {
        self.budget.enter_nested(operation, location)
    }

    /// Charges session-global algorithm work, fusing on refusal.
    pub fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_work(units, operation)
    }

    /// Permanently refuses a codec-local resource request.
    ///
    /// Codecs use this when a bounded recovery algorithm reaches a fixed
    /// local ceiling instead of a session-wide dimension. The refusal fuses
    /// the session so a caller cannot accidentally turn it into a semantic
    /// fallback or report success after the limit was reached.
    pub fn refuse_codec_limit(
        &self,
        operation: &'static str,
        limit: u64,
        requested: u64,
        location: Option<SourceLocation>,
    ) -> CodecError {
        self.budget.refuse(
            ResourceDimension::Codec(operation),
            ResourceFailure::BudgetExceeded,
            LimitScope::Global,
            limit,
            requested.min(limit),
            requested.saturating_sub(limit),
            operation,
            location,
        )
    }

    /// Creates a local work slice that also draws from the session allowance.
    pub fn work_budget(&self, local_limit: u64) -> WorkBudget<'_> {
        WorkBudget::for_session(local_limit, &self.budget)
    }

    // --- decompression ------------------------------------------------------

    /// Begins an expansion whose output is charged incrementally and becomes
    /// available only after successful finalization.
    pub fn begin_expand(
        &self,
        source: View<'_>,
        spec: ExpandSpec,
    ) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        self.begin_expand_as(source, spec, "expanded")
    }

    /// Begins a labeled expansion so the derived space resolves to `label`.
    pub fn begin_expand_as(
        &self,
        source: View<'_>,
        spec: ExpandSpec,
        label: impl Into<String>,
    ) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if let ExpandSpec::Exact(size) = spec {
            let per_expand = self.per_expand_allowance();
            if size > per_expand {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    LimitScope::PerExpand,
                    size,
                    "begin_expand",
                    Some(source.location()),
                ));
            }
            if size
                > self
                    .decompression_allowance()
                    .saturating_sub(self.budget.decompressed_used())
            {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    LimitScope::Global,
                    size,
                    "begin_expand",
                    Some(source.location()),
                ));
            }
        }
        let mut buffer: Vec<u8> = Vec::new();
        let reserve = match spec {
            ExpandSpec::Exact(size) => size.min(RESERVE_CLAMP),
            ExpandSpec::Unknown => 0,
        };
        if reserve > 0 {
            let reserve = usize::try_from(reserve).unwrap_or(usize::MAX);
            buffer.try_reserve(reserve).map_err(|_| {
                self.fuse(
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
            label: label.into(),
            source_space: source.space(),
            source_start: source.start() as u64,
            source_end: source.end() as u64,
            buffer,
            written: 0,
        })
    }

    /// Copies several input extents into one derived view.
    pub fn concat_views(&self, inputs: &[View<'_>]) -> Result<View<'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let location = inputs.first().copied().map(View::location);
        let total = inputs.iter().try_fold(0usize, |total, view| {
            total.checked_add(view.window().len()).ok_or_else(|| {
                self.budget.refuse(
                    ResourceDimension::RetainedBytes,
                    ResourceFailure::BudgetExceeded,
                    LimitScope::Global,
                    self.policy.limits.max_retained_bytes,
                    total as u64,
                    view.window().len() as u64,
                    "concat_views",
                    location,
                )
            })
        })?;
        let reservation = self.reserve_scoped(total as u64, "concat_views", location)?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget.refuse(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::AllocationFailed,
                LimitScope::Global,
                self.policy.limits.max_materialized_bytes,
                0,
                total as u64,
                "concat_views",
                location,
            )
        })?;
        for view in inputs {
            buffer.extend_from_slice(view.window());
        }
        let bytes = self.arena.alloc(buffer.into_boxed_slice());
        reservation.commit()?;
        let parents = inputs.iter().map(|view| view.space()).collect();
        let space = self.allocate_space("concat".into(), SpaceDerivation::Concatenated { parents });
        Ok(View::over_space(bytes, space))
    }

    /// Registers a stored (uncompressed) child range as a space that borrows
    /// the parent bytes without copying.
    ///
    /// `range` is expressed in the parent view's own space coordinates and must
    /// lie within the parent window; a range that escapes the parent is refused
    /// here, at the request site, exactly as [`View::child`] refuses. No bytes
    /// are copied. It is the archive-entry counterpart of [`DecodeContext::begin_expand`] —
    /// stored ZIP entries take this path, compressed ones take the expander.
    /// Registration still refuses on a fused context so a stored entry cannot be
    /// admitted after a refusal.
    pub fn register_slice<'v>(
        &self,
        parent: View<'v>,
        range: ByteRange,
    ) -> Result<View<'v>, CodecError> {
        self.register_slice_as(parent, range, "stored")
    }

    /// Registers a labeled stored child range so the space resolves to `label`.
    pub fn register_slice_as<'v>(
        &self,
        parent: View<'v>,
        range: ByteRange,
        label: impl Into<String>,
    ) -> Result<View<'v>, CodecError> {
        if let Some(limit) = self.budget.fused() {
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
        let space = self.allocate_space(
            label.into(),
            SpaceDerivation::StoredSlice {
                parent: parent.space(),
                range,
            },
        );
        Ok(View::over_space(child.window(), space))
    }

    // --- lifecycle ----------------------------------------------------------

    /// Closes a decode or inspection session, returning a fused resource error
    /// even when codec code swallowed the charge that caused it.
    pub fn finish_session(self) -> Result<(), CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        Ok(())
    }
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

/// How much output an expansion is expected to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSpec {
    /// A declared exact size, enforced per-write and at finalize.
    Exact(u64),
    /// No trustworthy declared size: the decompression limits apply.
    Unknown,
}

/// Writes decompressed output under incremental charging.
#[derive(Debug)]
pub struct ExpandWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    spec: ExpandSpec,
    location: SourceLocation,
    label: String,
    source_space: SpaceId,
    source_start: u64,
    source_end: u64,
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
            _ => {}
        }
        let per_expand = self.ctx.per_expand_allowance();
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "expand_write",
                Some(self.location),
            ));
        }
        self.ctx.charge_decompressed(
            LimitScope::Global,
            len,
            "expand_write",
            Some(self.location),
        )?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
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

    /// Finalizes the expansion, stores it in the arena, and registers its space.
    pub fn finalize(self) -> Result<View<'a>, CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written != size {
                return Err(CodecError::Malformed(format!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written
                )));
            }
        }
        let bytes = self.ctx.arena.alloc(self.buffer.into_boxed_slice());
        let space = self.ctx.allocate_space(
            self.label,
            SpaceDerivation::Expanded {
                parent: self.source_space,
                source_range: ByteRange {
                    start: self.source_start,
                    end: self.source_end,
                },
            },
        );
        Ok(View::over_space(bytes, space))
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        self.written
    }
}
